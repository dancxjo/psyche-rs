use crate::config::DistillerConfig;
use std::collections::HashMap;
use std::path::PathBuf;

/// Maps output kinds to Unix socket paths for distillers.
#[derive(Default)]
pub struct Router {
    map: HashMap<String, PathBuf>,
}

impl Router {
    /// Build a router from a list of distillers.
    pub fn from_configs(cfgs: &[DistillerConfig]) -> Self {
        let mut map = HashMap::new();
        for c in cfgs {
            let sock = PathBuf::from(format!("/run/psyche/{}.sock", c.name));
            map.insert(c.output.clone(), sock);
        }
        Self { map }
    }

    /// Resolve the socket for a memory kind.
    pub fn socket_for(&self, kind: &str) -> Option<&PathBuf> {
        self.map.get(kind)
    }
}
