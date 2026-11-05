use crate::domain::{config::Config, resources::ResourceSubject};
use std::path::Path;

impl Config {
    pub fn selection_active(&self) -> bool {
        !self.selected_paths.is_empty() || !self.selected_resources.is_empty()
    }

    pub fn path_owner(&self, path: &Path) -> &str {
        self.mounts
            .iter()
            .filter_map(|mount| {
                self.logical(&mount.path)
                    .ok()
                    .filter(|root| path.starts_with(root))
                    .map(|root| (root.components().count(), mount.name.as_str()))
            })
            .max_by_key(|(depth, _)| *depth)
            .map_or("default", |(_, name)| name)
    }

    pub fn path_selected(&self, path: &Path) -> bool {
        !self.selection_active()
            || self
                .selected_paths
                .iter()
                .any(|p| self.logical(p).is_ok_and(|p| path.starts_with(p)))
            || self.selected_resources.iter().any(|r| match &r.subject {
                ResourceSubject::Path { key } => self.path_for_key(key).is_ok_and(|p| p == path),
                _ => false,
            })
    }

    pub fn scope_selected(&self, name: &str, apply: bool) -> bool {
        if !self.selected_mounts.is_empty() && !self.selected_mounts.iter().any(|m| m == name) {
            return false;
        }
        if apply && self.selection_active() {
            return self
                .selected_paths
                .iter()
                .any(|p| self.logical(p).is_ok_and(|p| self.path_owner(&p) == name))
                || self.selected_resources.iter().any(|r| r.collection == name);
        }
        !apply
            || name == "default"
            || !self.selected_mounts.is_empty()
            || self.mounts.iter().any(|m| m.name == name && m.apply)
    }

    pub fn packages_selected(&self) -> bool {
        !self.selection_active()
            || self
                .selected_resources
                .iter()
                .any(|r| matches!(r.subject, ResourceSubject::Package { .. }))
    }

    pub fn package_selected(&self, name: &str) -> bool {
        !self.selection_active()
            || self
                .selected_resources
                .iter()
                .any(|r| matches!(&r.subject, ResourceSubject::Package { name: n } if n == name))
    }
}
