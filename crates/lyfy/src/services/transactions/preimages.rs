use crate::Context;
use crate::domain::{plan::Plan, state::Content};
use crate::infrastructure::{
    filesystem::Root,
    store::{Store, timestamp, write_json},
};
use crate::services::files::observe_like;
use anyhow::{Result, ensure};

pub fn before_packages(context: &Context, plan: &Plan) -> Result<()> {
    if plan.changes.is_empty() {
        return Ok(());
    }
    let directory = context
        .config
        .state_dir
        .join("package_preimages")
        .join(timestamp().to_string());
    let backup = Store::new(directory.clone());
    let root = Root::open(&context.config.root)?;
    for change in &plan.changes {
        let path = context.config.path_for_key(&change.key)?;
        let observed = observe_like(&context.config, &root, &path, change.before.as_ref())?;
        ensure!(
            observed.as_ref().map(|(n, _)| n) == change.before.as_ref(),
            "target changed before package execution: {}",
            change.key
        );
        if let Some((node, Some(mut file))) = observed {
            let Content::File { sha256 } = node.content else {
                unreachable!()
            };
            backup.put_file(&sha256, &mut file)?;
        }
    }
    #[derive(serde::Serialize)]
    struct Journal<'a> {
        schema: u32,
        status: &'a str,
        root: &'a std::path::Path,
        home_dir: &'a std::path::Path,
        plan: &'a Plan,
    }
    write_json(
        &directory.join("journal.json"),
        &Journal {
            schema: 1,
            status: "preimages_before_package_actions",
            root: &context.config.root,
            home_dir: &context.config.home_dir,
            plan,
        },
    )
}
