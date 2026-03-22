use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Default, Clone)]
pub(crate) struct WorkspaceGraph {
    deps_by_module: BTreeMap<String, BTreeSet<String>>,
}

impl WorkspaceGraph {
    pub(crate) fn with_modules(modules: impl IntoIterator<Item = String>) -> Self {
        let mut deps_by_module = BTreeMap::new();
        for module in modules {
            deps_by_module.insert(module, BTreeSet::new());
        }
        Self { deps_by_module }
    }

    pub(crate) fn add_dependency(
        &mut self,
        module: &str,
        depends_on: &str,
    ) -> Result<(), GraphError> {
        if !self.deps_by_module.contains_key(module) {
            return Err(GraphError::UnknownModule(module.to_owned()));
        }
        if !self.deps_by_module.contains_key(depends_on) {
            return Err(GraphError::UnknownModule(depends_on.to_owned()));
        }
        self.deps_by_module
            .entry(module.to_owned())
            .or_default()
            .insert(depends_on.to_owned());
        Ok(())
    }

    pub(crate) fn modules(&self) -> BTreeSet<String> {
        self.deps_by_module.keys().cloned().collect()
    }

    pub(crate) fn dependency_closure(&self, root: &str) -> Result<BTreeSet<String>, GraphError> {
        if !self.deps_by_module.contains_key(root) {
            return Err(GraphError::UnknownModule(root.to_owned()));
        }

        let mut selected = BTreeSet::new();
        let mut stack = vec![root.to_owned()];

        while let Some(next) = stack.pop() {
            if !selected.insert(next.clone()) {
                continue;
            }

            let Some(dependencies) = self.deps_by_module.get(&next) else {
                return Err(GraphError::UnknownModule(next));
            };

            for dependency in dependencies {
                stack.push(dependency.clone());
            }
        }

        Ok(selected)
    }

    pub(crate) fn topological_order(&self) -> Result<Vec<String>, GraphError> {
        let mut incoming = self
            .deps_by_module
            .iter()
            .map(|(module, deps)| (module.clone(), deps.len()))
            .collect::<BTreeMap<_, _>>();

        let mut dependents = self
            .deps_by_module
            .keys()
            .map(|module| (module.clone(), Vec::<String>::new()))
            .collect::<BTreeMap<_, _>>();

        for (module, dependencies) in &self.deps_by_module {
            for dependency in dependencies {
                dependents
                    .entry(dependency.clone())
                    .or_default()
                    .push(module.clone());
            }
        }

        let mut ready = incoming
            .iter()
            .filter_map(|(module, degree)| (*degree == 0).then_some(module.clone()))
            .collect::<Vec<_>>();
        ready.sort();

        let mut order = Vec::new();
        while let Some(module) = ready.pop() {
            order.push(module.clone());
            let mut neighbors = dependents.remove(&module).unwrap_or_default();
            neighbors.sort();
            neighbors.reverse();

            for dependent in neighbors {
                if let Some(value) = incoming.get_mut(&dependent) {
                    *value = value.saturating_sub(1);
                    if *value == 0 {
                        ready.push(dependent);
                    }
                }
            }
        }

        if order.len() != self.deps_by_module.len() {
            return Err(GraphError::CycleDetected);
        }

        Ok(order)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GraphError {
    UnknownModule(String),
    CycleDetected,
}

#[cfg(test)]
mod tests {
    use super::WorkspaceGraph;

    #[test]
    fn topological_order_builds_dependencies_first() {
        let mut graph = WorkspaceGraph::with_modules([
            "app".to_owned(),
            "domain".to_owned(),
            "shared".to_owned(),
        ]);
        graph.add_dependency("app", "domain").expect("valid edge");
        graph
            .add_dependency("domain", "shared")
            .expect("valid edge");

        let order = graph.topological_order().expect("order should exist");

        let app = order.iter().position(|value| value == "app").unwrap_or(0);
        let domain = order
            .iter()
            .position(|value| value == "domain")
            .unwrap_or(0);
        let shared = order
            .iter()
            .position(|value| value == "shared")
            .unwrap_or(0);

        assert!(shared < domain);
        assert!(domain < app);
    }

    #[test]
    fn closure_collects_transitive_dependencies() {
        let mut graph = WorkspaceGraph::with_modules([
            "api".to_owned(),
            "domain".to_owned(),
            "util".to_owned(),
            "cli".to_owned(),
        ]);
        graph.add_dependency("api", "domain").expect("valid edge");
        graph.add_dependency("domain", "util").expect("valid edge");

        let selected = graph
            .dependency_closure("api")
            .expect("closure should succeed");

        assert!(selected.contains("api"));
        assert!(selected.contains("domain"));
        assert!(selected.contains("util"));
        assert!(!selected.contains("cli"));
    }

    #[test]
    fn cycle_detection_surfaces_error() {
        let mut graph = WorkspaceGraph::with_modules(["a".to_owned(), "b".to_owned()]);
        graph.add_dependency("a", "b").expect("valid edge");
        graph.add_dependency("b", "a").expect("valid edge");

        let error = graph
            .topological_order()
            .expect_err("cycle should fail ordering");

        assert!(matches!(error, super::GraphError::CycleDetected));
    }
}
