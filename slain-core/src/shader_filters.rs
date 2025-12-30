//! Registry for custom shader-based filters.

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ShaderFilterSpec {
    pub name: String,
    pub source: String,
    pub entry_point: String,
}

impl ShaderFilterSpec {
    pub fn new(name: impl Into<String>, source: impl Into<String>, entry_point: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            source: source.into(),
            entry_point: entry_point.into(),
        }
    }
}

#[derive(Default)]
pub struct ShaderFilterRegistry {
    filters: HashMap<String, ShaderFilterSpec>,
}

impl ShaderFilterRegistry {
    pub fn new() -> Self {
        Self {
            filters: HashMap::new(),
        }
    }

    pub fn register(&mut self, spec: ShaderFilterSpec) {
        self.filters.insert(spec.name.clone(), spec);
    }

    pub fn get(&self, name: &str) -> Option<&ShaderFilterSpec> {
        self.filters.get(name)
    }

    pub fn list(&self) -> Vec<String> {
        self.filters.keys().cloned().collect()
    }
}
