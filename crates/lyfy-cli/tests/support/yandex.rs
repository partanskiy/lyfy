use crate::http::{Request, Response, Server};
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

#[derive(Default)]
pub struct State {
    pub nodes: BTreeMap<String, Option<Vec<u8>>>,
    pub fail_upload: bool,
    pub reject_marker: bool,
    pub lose_marker_response: bool,
    pub duplicate_create: bool,
    pub extra_pages: usize,
    pub pending_polls: usize,
}

pub struct Disk {
    pub server: Server,
    pub state: Arc<Mutex<State>>,
}

fn missing() -> Response {
    Response::json(404, json!({"error":"DiskNotFoundError"}))
}
fn conflict() -> Response {
    Response::json(409, json!({"error":"DiskResourceAlreadyExistsError"}))
}
fn link(endpoint: &str, path: &str) -> String {
    format!(
        "{endpoint}/data?path={}&ticket=fixture-capability",
        percent_encoding::utf8_percent_encode(path, percent_encoding::NON_ALPHANUMERIC)
    )
}

fn handle(request: Request, endpoint: &str, state: &Mutex<State>) -> Option<Response> {
    let mut state = state.lock().unwrap();
    let path = request.query.get("path").cloned().unwrap_or_default();
    if request.path == "/data" {
        assert!(!request.headers.contains_key("authorization"));
        assert!(
            request
                .query
                .get("ticket")
                .is_some_and(|v| v == "fixture-capability")
        );
        return Some(match request.method.as_str() {
            "GET" => state
                .nodes
                .get(&path)
                .and_then(|n| n.as_ref())
                .map(|bytes| Response::bytes(200, bytes.clone()))
                .unwrap_or_else(missing),
            "PUT" => {
                if path.contains("/.pending/") && state.fail_upload {
                    state.fail_upload = false;
                    Response::json(500, json!({"error":"interrupted"}))
                } else {
                    state.nodes.insert(path, Some(request.body));
                    Response::bytes(201, vec![])
                }
            }
            _ => Response::bytes(405, vec![]),
        });
    }
    assert!(
        request
            .headers
            .get("authorization")
            .is_some_and(|v| v == "OAuth fixture-oauth")
    );
    if request.path == "/v1/disk/operations/delete" {
        state.pending_polls += 1;
        return Some(Response::json(
            200,
            json!({"status":if state.pending_polls % 2 == 1 {"in-progress"} else {"success"}}),
        ));
    }
    if request.path == "/v1/disk/resources/download" {
        return Some(if state.nodes.get(&path).is_some_and(|n| n.is_some()) {
            Response::json(
                200,
                json!({"href":link(endpoint,&path),"method":"GET","templated":false}),
            )
        } else {
            missing()
        });
    }
    if request.path == "/v1/disk/resources/upload" {
        if state.nodes.contains_key(&path) {
            return Some(conflict());
        }
        state.nodes.insert(path.clone(), Some(vec![]));
        return Some(Response::json(
            200,
            json!({"href":link(endpoint,&path),"method":"PUT","templated":false}),
        ));
    }
    assert_eq!(request.path, "/v1/disk/resources");
    Some(match request.method.as_str() {
        "GET" => {
            match state.nodes.get(&path) {
                None => missing(),
                Some(Some(bytes)) => Response::json(
                    200,
                    json!({"name":path.rsplit('/').next(),"type":"file","size":bytes.len()}),
                ),
                Some(None) => {
                    let prefix = format!("{path}/");
                    let items: Vec<_> = state.nodes.iter().filter_map(|(key,value)| { let name = key.strip_prefix(&prefix)?; (!name.contains('/')).then(|| json!({"name":name,"type":if value.is_none() {"dir"} else {"file"}})) }).collect();
                    let offset: usize = request
                        .query
                        .get("offset")
                        .map(|s| s.parse().unwrap())
                        .unwrap_or(0);
                    if offset > 0 {
                        state.extra_pages += 1;
                    }
                    Response::json(
                        200,
                        json!({"type":"dir","_embedded":{"items":items.iter().skip(offset).take(2).collect::<Vec<_>>(),"limit":2,"offset":offset,"total":items.len()}}),
                    )
                }
            }
        }
        "PUT" => {
            let marker = path.contains("/.pointers/") && path.contains("/HEAD/");
            if marker && state.reject_marker {
                state.reject_marker = false;
                return Some(Response::json(500, json!({"error":"publication rejected"})));
            }
            if state.nodes.contains_key(&path) && !state.duplicate_create {
                conflict()
            } else {
                let parent = path.rsplit_once('/').unwrap().0;
                if !state.nodes.get(parent).is_some_and(|n| n.is_none()) {
                    missing()
                } else {
                    state.nodes.insert(path, None);
                    if marker && state.lose_marker_response {
                        state.lose_marker_response = false;
                        return None;
                    }
                    Response::json(201, json!({}))
                }
            }
        }
        "DELETE" => {
            state
                .nodes
                .retain(|key, _| key != &path && !key.starts_with(&format!("{path}/")));
            state.pending_polls = 0;
            Response::json(
                202,
                json!({"href":format!("{endpoint}/v1/disk/operations/delete"),"method":"GET","templated":false}),
            )
        }
        _ => Response::bytes(405, vec![]),
    })
}

impl Disk {
    pub fn start() -> Option<Self> {
        let state = Arc::new(Mutex::new(State::default()));
        state.lock().unwrap().nodes.insert("app:".into(), None);
        let data = Arc::clone(&state);
        let server = Server::start(move |r, e| handle(r, e, &data))?;
        Some(Self { server, state })
    }
}
