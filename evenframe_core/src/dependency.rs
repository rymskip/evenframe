use crate::evenframe_log;
use crate::schemasync::TableConfig;
use crate::types::{FieldType, StructConfig, TaggedUnion, VariantData};
use convert_case::{Case, Casing};
use petgraph::algo::toposort;
use petgraph::{algo::kosaraju_scc, graphmap::DiGraphMap};
use std::collections::{HashMap, HashSet};
use tracing;

/// A helper struct to track recursion information for types
#[derive(Debug)]
pub struct RecursionInfo {
    /// `type_name -> scc_id`
    pub comp_of: HashMap<String, usize>,
    /// `scc_id -> { "is_recursive": bool, "members": Vec<String> }`
    pub meta: HashMap<usize, (bool, Vec<String>)>,
}

impl RecursionInfo {
    /// Returns true when current & target are in the **same** SCC and that SCC
    /// is either larger than 1 **or** has a self-loop
    pub fn is_recursive_pair(&self, current: &str, target: &str) -> bool {
        let c_id = self.comp_of.get(current);
        let t_id = self.comp_of.get(target);
        match (c_id, t_id) {
            (Some(c), Some(t)) if c == t => self.meta[c].0, // same comp & recursive
            _ => false,
        }
    }
}

/// Build the dependency graph from your `FieldType` tree and analyze recursion
pub fn analyse_recursion(
    structs: &HashMap<String, StructConfig>,
    enums: &HashMap<String, TaggedUnion>,
) -> RecursionInfo {
    let known: HashSet<_> = structs
        .values()
        .map(|struct_config| struct_config.struct_name.to_case(Case::Pascal))
        .chain(enums.values().map(|e| e.enum_name.to_case(Case::Pascal)))
        .collect();

    let mut deps: HashMap<String, HashSet<String>> = HashMap::new();

    for struct_config in structs.values() {
        let from = struct_config.struct_name.to_case(Case::Pascal);
        let entry = deps.entry(from.clone()).or_default();
        for f in &struct_config.fields {
            collect_refs(&f.field_type, &known, entry);
        }
    }
    tracing::debug!("Collecting dependencies from enums");
    for e in enums.values() {
        let from = e.enum_name.to_case(Case::Pascal);
        tracing::trace!(enum_name = %from, "Processing enum dependencies");
        let entry = deps.entry(from.clone()).or_default();
        for v in &e.variants {
            if let Some(variant_data) = &v.data {
                let variant_data_field_type = match variant_data {
                    VariantData::InlineStruct(enum_struct) => {
                        &FieldType::Other(enum_struct.struct_name.clone())
                    }
                    VariantData::DataStructureRef(field_type) => field_type,
                };
                collect_refs(variant_data_field_type, &known, entry);
            }
        }
    }

    // Build graph
    tracing::debug!("Building dependency graph");
    let mut g: DiGraphMap<&str, ()> = DiGraphMap::new();
    for (from, tos) in &deps {
        // ensure node exists even if it has no outgoing edges
        g.add_node(from.as_str());
        for to in tos {
            g.add_edge(from.as_str(), to.as_str(), ());
        }
    }
    tracing::trace!(
        node_count = g.node_count(),
        edge_count = g.edge_count(),
        "Graph built"
    );

    // Strongly connected components
    tracing::debug!("Finding strongly connected components");
    let sccs = kosaraju_scc(&g); // Vec<Vec<&str>>
    tracing::debug!(scc_count = sccs.len(), "SCCs found");

    let mut comp_of = HashMap::<String, usize>::new();
    let mut meta = HashMap::<usize, (bool, Vec<String>)>::new();

    for (idx, comp) in sccs.iter().enumerate() {
        let self_loop = comp.len() == 1 && g.contains_edge(comp[0], comp[0]);
        let recursive = self_loop || comp.len() > 1;
        let members = comp.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
        for m in &members {
            comp_of.insert(m.clone(), idx);
        }
        meta.insert(idx, (recursive, members));
    }

    RecursionInfo { comp_of, meta }
}

/// Returns the set of **direct** dependencies of a type name
/// (other structs / enums that it references in its fields or variants).
pub fn deps_of(
    name: &str,
    structs: &HashMap<String, StructConfig>,
    enums: &HashMap<String, TaggedUnion>,
) -> HashSet<String> {
    tracing::trace!(name = %name, "Getting direct dependencies of type");
    // Build a quick "known-types" set so we don't count primitives.
    let known: HashSet<_> = structs
        .values()
        .map(|struct_config| struct_config.struct_name.to_case(Case::Pascal))
        .chain(enums.values().map(|e| e.enum_name.to_case(Case::Pascal)))
        .collect();

    let mut acc = HashSet::new();

    // If `name` is a struct, walk its fields
    if let Some(struct_config) = structs
        .values()
        .find(|struct_config| struct_config.struct_name.to_case(Case::Pascal) == name)
    {
        tracing::trace!(struct_name = %struct_config.struct_name, field_count = struct_config.fields.len(), "Walking struct fields for dependencies");
        for f in &struct_config.fields {
            collect_refs(&f.field_type, &known, &mut acc);
        }
    }

    // If `name` is an enum, walk its variants
    if let Some(e) = enums
        .values()
        .find(|e| e.enum_name.to_case(Case::Pascal) == name)
    {
        tracing::trace!(enum_name = %e.enum_name, variant_count = e.variants.len(), "Walking enum variants for dependencies");
        for v in &e.variants {
            if let Some(variant_data) = &v.data {
                let variant_data_field_type = match variant_data {
                    VariantData::InlineStruct(enum_struct) => {
                        &FieldType::Other(enum_struct.struct_name.clone())
                    }
                    VariantData::DataStructureRef(field_type) => field_type,
                };
                collect_refs(variant_data_field_type, &known, &mut acc);
            }
        }
    }

    tracing::trace!(dependency_count = acc.len(), "Dependencies collected");
    acc
}

/// Collect references to other types from a FieldType
pub fn collect_refs(ft: &FieldType, known: &HashSet<String>, acc: &mut HashSet<String>) {
    tracing::trace!(field_type = ?ft, "Collecting references from field type");
    use FieldType::*;
    match ft {
        Tuple(v) => v.iter().for_each(|f| collect_refs(f, known, acc)),
        Struct(v) => v.iter().for_each(|(_, f)| collect_refs(f, known, acc)),
        Option(i) | Vec(i) | RecordLink(i) => collect_refs(i, known, acc),
        HashMap(k, v) | BTreeMap(k, v) => {
            collect_refs(k, known, acc);
            collect_refs(v, known, acc);
        }
        Other(name) if known.contains(name) => {
            acc.insert(name.clone());
        }
        _ => {}
    }
}

/// Analyze recursion specifically for tables (TableConfig)
/// This is a specialized version that only considers table dependencies
pub fn analyse_recursion_tables(
    tables: &HashMap<String, crate::schemasync::TableConfig>,
) -> RecursionInfo {
    tracing::info!(
        table_count = tables.len(),
        "Analyzing recursion in table dependencies"
    );
    // Convert tables to structs for analysis
    let structs: HashMap<String, StructConfig> = tables
        .iter()
        .map(|(name, table)| (name.clone(), table.struct_config.clone()))
        .collect();
    tracing::debug!("Converted tables to structs for analysis");

    // Tables don't have enums, so pass empty map
    let enums = HashMap::new();

    // Use the regular analyse_recursion with converted data
    tracing::debug!("Delegating to main recursion analyzer");
    analyse_recursion(&structs, &enums)
}

/// Get dependencies of a table by analyzing its struct config
pub fn deps_of_table(
    table_name: &str,
    tables: &HashMap<String, crate::schemasync::TableConfig>,
) -> HashSet<String> {
    tracing::debug!(table_name = %table_name, "Getting dependencies of table");
    // Build set of known table names in PascalCase
    let known: HashSet<_> = tables.keys().map(|s| s.to_case(Case::Pascal)).collect();
    tracing::trace!(
        known_table_count = known.len(),
        "Built set of known table names"
    );

    // Build a map from PascalCase to original table names
    let pascal_to_original: HashMap<String, String> = tables
        .keys()
        .map(|k| (k.to_case(Case::Pascal), k.clone()))
        .collect();

    let mut acc = HashSet::new();

    // Find the table and analyze its fields
    if let Some(table) = tables.get(table_name) {
        tracing::trace!(
            table = %table_name,
            field_count = table.struct_config.fields.len(),
            "Analyzing table fields for dependencies"
        );
        for field in &table.struct_config.fields {
            collect_refs(&field.field_type, &known, &mut acc);
        }
    } else {
        tracing::warn!(table_name = %table_name, "Table not found");
    }

    // Convert PascalCase dependencies back to original table names
    let result: HashSet<String> = acc
        .into_iter()
        .filter_map(|pascal_name| pascal_to_original.get(&pascal_name).cloned())
        .collect();
    tracing::debug!(
        dependency_count = result.len(),
        "Table dependencies collected"
    );
    result
}

/// Collect all dependencies of a table including nested objects and enums
fn collect_table_dependencies(
    table_name: &str,
    tables: &HashMap<String, TableConfig>,
    objects: &HashMap<String, StructConfig>,
    enums: &HashMap<String, TaggedUnion>,
    visited_types: &mut HashSet<String>,
) -> HashSet<String> {
    tracing::trace!(
        table_name = %table_name,
        "Collecting all dependencies of table including nested objects"
    );
    let mut dependencies = HashSet::new();

    // Get the table configuration
    if let Some(table) = tables.get(table_name) {
        // If this is a relation table, it depends on both the 'from' and 'to' tables
        if let Some(relation) = &table.relation {
            tracing::trace!(
                table = %table_name,
                from = ?relation.from,
                to = ?relation.to,
                "Processing relation table dependencies"
            );
            // Add dependency on each 'from' table
            for from_table in &relation.from {
                let from_snake = from_table.to_case(Case::Snake);
                if tables.contains_key(from_table) {
                    dependencies.insert(from_table.clone());
                } else if tables.contains_key(&from_snake) {
                    dependencies.insert(from_snake);
                }
            }

            // Add dependency on each 'to' table
            for to_table in &relation.to {
                let to_snake = to_table.to_case(Case::Snake);
                if tables.contains_key(to_table) {
                    dependencies.insert(to_table.clone());
                } else if tables.contains_key(&to_snake) {
                    dependencies.insert(to_snake);
                }
            }
        }

        // Analyze each field in the table
        tracing::trace!(
            field_count = table.struct_config.fields.len(),
            "Analyzing table fields"
        );
        for field in &table.struct_config.fields {
            collect_field_type_dependencies(
                &field.field_type,
                tables,
                objects,
                enums,
                &mut dependencies,
                visited_types,
            );
        }
    }

    tracing::trace!(
        table = %table_name,
        dependency_count = dependencies.len(),
        "Table dependencies collection complete"
    );
    dependencies
}

/// Recursively collect dependencies from a field type
pub fn collect_field_type_dependencies(
    field_type: &FieldType,
    tables: &HashMap<String, TableConfig>,
    objects: &HashMap<String, StructConfig>,
    enums: &HashMap<String, TaggedUnion>,
    dependencies: &mut HashSet<String>,
    visited_types: &mut HashSet<String>,
) {
    tracing::trace!(field_type = ?field_type, "Collecting field type dependencies");
    match field_type {
        FieldType::Other(type_name) => {
            // Avoid infinite recursion
            if visited_types.contains(type_name) {
                tracing::trace!(type_name = %type_name, "Type already visited, skipping to avoid recursion");
                return;
            }
            visited_types.insert(type_name.clone());
            tracing::trace!(type_name = %type_name, "Processing Other type");

            let snake_case_name = type_name.to_case(Case::Snake);

            // Check if it's a table reference
            if tables.contains_key(type_name) {
                tracing::trace!(type_name = %type_name, "Found table reference");
                dependencies.insert(type_name.clone());
            } else if tables.contains_key(&snake_case_name) {
                tracing::trace!(type_name = %snake_case_name, "Found table reference (snake case)");
                dependencies.insert(snake_case_name.clone());
            }

            // Check if it's an object/struct and recursively analyze its fields
            if let Some(obj) = objects
                .get(type_name)
                .or_else(|| objects.get(&snake_case_name))
            {
                tracing::trace!(
                    type_name = %type_name,
                    field_count = obj.fields.len(),
                    "Found object/struct, analyzing fields"
                );
                for field in &obj.fields {
                    collect_field_type_dependencies(
                        &field.field_type,
                        tables,
                        objects,
                        enums,
                        dependencies,
                        visited_types,
                    );
                }
            }

            // Check if it's an enum and analyze its variants
            if let Some(enum_def) = enums.get(type_name).or_else(|| enums.get(&snake_case_name)) {
                tracing::trace!(
                    type_name = %type_name,
                    variant_count = enum_def.variants.len(),
                    "Found enum, analyzing variants"
                );
                for variant in &enum_def.variants {
                    if let Some(variant_data) = &variant.data {
                        match variant_data {
                            VariantData::InlineStruct(enum_struct) => {
                                // Recursively analyze inline struct
                                if let Some(obj) = objects.get(&enum_struct.struct_name) {
                                    for field in &obj.fields {
                                        collect_field_type_dependencies(
                                            &field.field_type,
                                            tables,
                                            objects,
                                            enums,
                                            dependencies,
                                            visited_types,
                                        );
                                    }
                                }
                            }
                            VariantData::DataStructureRef(ref_type) => {
                                collect_field_type_dependencies(
                                    ref_type,
                                    tables,
                                    objects,
                                    enums,
                                    dependencies,
                                    visited_types,
                                );
                            }
                        }
                    }
                }
            }
        }
        FieldType::Option(inner) | FieldType::Vec(inner) | FieldType::RecordLink(inner) => {
            collect_field_type_dependencies(
                inner,
                tables,
                objects,
                enums,
                dependencies,
                visited_types,
            );
        }
        FieldType::Tuple(types) => {
            for t in types {
                collect_field_type_dependencies(
                    t,
                    tables,
                    objects,
                    enums,
                    dependencies,
                    visited_types,
                );
            }
        }
        FieldType::Struct(fields) => {
            for (_, field_type) in fields {
                collect_field_type_dependencies(
                    field_type,
                    tables,
                    objects,
                    enums,
                    dependencies,
                    visited_types,
                );
            }
        }
        FieldType::HashMap(key_type, value_type) | FieldType::BTreeMap(key_type, value_type) => {
            collect_field_type_dependencies(
                key_type,
                tables,
                objects,
                enums,
                dependencies,
                visited_types,
            );
            collect_field_type_dependencies(
                value_type,
                tables,
                objects,
                enums,
                dependencies,
                visited_types,
            );
        }
        _ => {} // Primitive types
    }
}

/// Sort tables by dependencies using topological sort with SCC handling
pub fn sort_tables_by_dependencies(
    tables: &HashMap<String, TableConfig>,
    objects: &HashMap<String, StructConfig>,
    enums: &HashMap<String, TaggedUnion>,
) -> Vec<String> {
    tracing::info!(
        table_count = tables.len(),
        object_count = objects.len(),
        enum_count = enums.len(),
        "Sorting tables by dependencies"
    );
    // Build complete dependency graph including nested objects and enums
    let mut dependency_graph: HashMap<String, HashSet<String>> = HashMap::new();

    tracing::debug!("Building dependency graph for all tables");
    for table_name in tables.keys() {
        let mut visited_types = HashSet::new();
        let dependencies =
            collect_table_dependencies(table_name, tables, objects, enums, &mut visited_types);
        dependency_graph.insert(table_name.clone(), dependencies.clone());

        // Log dependencies for debugging
        if !dependencies.is_empty() {
            evenframe_log!(
                &format!("Table '{}' depends on: {:?}", table_name, &dependencies),
                "results.log",
                true
            );
        }
    }

    // Build petgraph for topological sorting
    tracing::debug!("Building petgraph for topological sorting");
    let mut graph = DiGraphMap::<&str, ()>::new();

    // Add all nodes first
    for table_name in tables.keys() {
        graph.add_node(table_name.as_str());
    }

    // Add edges (A depends on B = edge from A to B)
    for (table_name, dependencies) in &dependency_graph {
        for dep in dependencies {
            if tables.contains_key(dep) {
                graph.add_edge(table_name.as_str(), dep.as_str(), ());
            }
        }
    }

    // Detect strongly connected components for circular dependencies
    tracing::debug!("Detecting strongly connected components");
    let sccs = petgraph::algo::kosaraju_scc(&graph);
    tracing::info!(
        scc_count = sccs.len(),
        "Found strongly connected components"
    );

    // Build condensation graph (DAG of SCCs)
    tracing::debug!("Building condensation graph");
    let mut scc_map: HashMap<&str, usize> = HashMap::new();
    for (idx, scc) in sccs.iter().enumerate() {
        for node in scc {
            scc_map.insert(*node, idx);
        }
    }

    let mut condensation = DiGraphMap::<usize, ()>::new();
    for (from, tos) in &dependency_graph {
        if let Some(&from_scc) = scc_map.get(from.as_str()) {
            for to in tos {
                if let Some(&to_scc) = scc_map.get(to.as_str())
                    && from_scc != to_scc
                {
                    condensation.add_edge(from_scc, to_scc, ());
                }
            }
        }
    }

    // Topological sort of SCCs
    tracing::debug!("Performing topological sort of SCCs");
    let sorted_sccs = match toposort(&condensation, None) {
        Ok(order) => {
            tracing::debug!("Topological sort successful");
            order
        }
        Err(_) => {
            // If there's still a cycle (shouldn't happen with SCC), fall back to arbitrary order
            tracing::warn!("Cycle detected in SCC condensation graph, using arbitrary order");
            evenframe_log!(
                "Warning: Cycle detected in SCC condensation graph",
                "results.log",
                true
            );
            (0..sccs.len()).collect()
        }
    };

    // Build final sorted list
    tracing::debug!("Building final sorted table list");
    let mut result = Vec::new();
    let mut processed_tables = HashSet::new();

    // Process SCCs in reverse topological order (dependencies first)
    for scc_idx in sorted_sccs.into_iter().rev() {
        tracing::trace!(scc_idx = scc_idx, "Processing SCC");
        // Find all tables in this SCC
        let mut scc_tables: Vec<String> = tables
            .keys()
            .filter(|name| scc_map.get(name.as_str()) == Some(&scc_idx))
            .cloned()
            .collect();

        // Sort within SCC for deterministic output
        scc_tables.sort();

        // Log SCC info if it contains multiple tables
        if scc_tables.len() > 1 {
            tracing::warn!(
                tables = ?scc_tables,
                "Circular dependency detected among tables"
            );
            evenframe_log!(
                &format!(
                    "Circular dependency detected among tables: {:?}",
                    scc_tables
                ),
                "results.log",
                true
            );
        }

        for table in &scc_tables {
            processed_tables.insert(table.clone());
        }
        result.extend(scc_tables);
    }

    // Add any tables that weren't in the graph (isolated nodes with no dependencies)
    let mut missing_tables: Vec<String> = tables
        .keys()
        .filter(|name| !processed_tables.contains(*name))
        .cloned()
        .collect();
    missing_tables.sort();

    if !missing_tables.is_empty() {
        tracing::debug!(
            table_count = missing_tables.len(),
            "Found tables with no dependencies"
        );
        evenframe_log!(
            &format!(
                "Tables with no dependencies (adding at beginning): {:?}",
                missing_tables
            ),
            "results.log",
            true
        );
        // Add tables with no dependencies at the beginning
        result = missing_tables.into_iter().chain(result).collect();
    }

    tracing::info!(
        table_count = result.len(),
        "Table dependency sorting complete"
    );
    evenframe_log!(
        &format!("Final sorted table order: {:?}", result),
        "results.log",
        true
    );

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FieldType, StructConfig, StructField, TaggedUnion, Variant, VariantData};

    // ==================== RecursionInfo Tests ====================

    #[test]
    fn test_recursion_info_is_recursive_pair_same_recursive_component() {
        let mut comp_of = HashMap::new();
        comp_of.insert("TypeA".to_string(), 0);
        comp_of.insert("TypeB".to_string(), 0);

        let mut meta = HashMap::new();
        meta.insert(0, (true, vec!["TypeA".to_string(), "TypeB".to_string()]));

        let info = RecursionInfo { comp_of, meta };

        // Same component and recursive
        assert!(info.is_recursive_pair("TypeA", "TypeB"));
        assert!(info.is_recursive_pair("TypeB", "TypeA"));
    }

    #[test]
    fn test_recursion_info_is_recursive_pair_same_non_recursive_component() {
        let mut comp_of = HashMap::new();
        comp_of.insert("TypeA".to_string(), 0);

        let mut meta = HashMap::new();
        meta.insert(0, (false, vec!["TypeA".to_string()]));

        let info = RecursionInfo { comp_of, meta };

        // Same component but not recursive
        assert!(!info.is_recursive_pair("TypeA", "TypeA"));
    }

    #[test]
    fn test_recursion_info_is_recursive_pair_different_components() {
        let mut comp_of = HashMap::new();
        comp_of.insert("TypeA".to_string(), 0);
        comp_of.insert("TypeB".to_string(), 1);

        let mut meta = HashMap::new();
        meta.insert(0, (true, vec!["TypeA".to_string()]));
        meta.insert(1, (true, vec!["TypeB".to_string()]));

        let info = RecursionInfo { comp_of, meta };

        // Different components
        assert!(!info.is_recursive_pair("TypeA", "TypeB"));
    }

    #[test]
    fn test_recursion_info_is_recursive_pair_unknown_type() {
        let comp_of = HashMap::new();
        let meta = HashMap::new();
        let info = RecursionInfo { comp_of, meta };

        // Unknown types
        assert!(!info.is_recursive_pair("Unknown", "Other"));
    }

    #[test]
    fn test_recursion_info_is_recursive_pair_one_unknown() {
        let mut comp_of = HashMap::new();
        comp_of.insert("TypeA".to_string(), 0);

        let mut meta = HashMap::new();
        meta.insert(0, (true, vec!["TypeA".to_string()]));

        let info = RecursionInfo { comp_of, meta };

        // One known, one unknown
        assert!(!info.is_recursive_pair("TypeA", "Unknown"));
        assert!(!info.is_recursive_pair("Unknown", "TypeA"));
    }

    // ==================== collect_refs Tests ====================

    fn create_struct_field(name: &str, field_type: FieldType) -> StructField {
        StructField {
            field_name: name.to_string(),
            field_type,
            edge_config: None,
            define_config: None,
            format: None,
            validators: Vec::new(),
            always_regenerate: false,
        }
    }

    #[test]
    fn test_collect_refs_primitive_types() {
        let known: HashSet<String> = ["KnownType".to_string()].into_iter().collect();
        let mut acc = HashSet::new();

        collect_refs(&FieldType::String, &known, &mut acc);
        collect_refs(&FieldType::I32, &known, &mut acc);
        collect_refs(&FieldType::Bool, &known, &mut acc);
        collect_refs(&FieldType::F64, &known, &mut acc);

        assert!(acc.is_empty());
    }

    #[test]
    fn test_collect_refs_other_known_type() {
        let known: HashSet<String> = ["KnownType".to_string()].into_iter().collect();
        let mut acc = HashSet::new();

        collect_refs(&FieldType::Other("KnownType".to_string()), &known, &mut acc);

        assert!(acc.contains("KnownType"));
        assert_eq!(acc.len(), 1);
    }

    #[test]
    fn test_collect_refs_other_unknown_type() {
        let known: HashSet<String> = ["KnownType".to_string()].into_iter().collect();
        let mut acc = HashSet::new();

        collect_refs(
            &FieldType::Other("UnknownType".to_string()),
            &known,
            &mut acc,
        );

        assert!(acc.is_empty());
    }

    #[test]
    fn test_collect_refs_option_with_known_type() {
        let known: HashSet<String> = ["InnerType".to_string()].into_iter().collect();
        let mut acc = HashSet::new();

        collect_refs(
            &FieldType::Option(Box::new(FieldType::Other("InnerType".to_string()))),
            &known,
            &mut acc,
        );

        assert!(acc.contains("InnerType"));
    }

    #[test]
    fn test_collect_refs_vec_with_known_type() {
        let known: HashSet<String> = ["ElementType".to_string()].into_iter().collect();
        let mut acc = HashSet::new();

        collect_refs(
            &FieldType::Vec(Box::new(FieldType::Other("ElementType".to_string()))),
            &known,
            &mut acc,
        );

        assert!(acc.contains("ElementType"));
    }

    #[test]
    fn test_collect_refs_record_link() {
        let known: HashSet<String> = ["LinkedType".to_string()].into_iter().collect();
        let mut acc = HashSet::new();

        collect_refs(
            &FieldType::RecordLink(Box::new(FieldType::Other("LinkedType".to_string()))),
            &known,
            &mut acc,
        );

        assert!(acc.contains("LinkedType"));
    }

    #[test]
    fn test_collect_refs_tuple() {
        let known: HashSet<String> = ["TypeA".to_string(), "TypeB".to_string()]
            .into_iter()
            .collect();
        let mut acc = HashSet::new();

        collect_refs(
            &FieldType::Tuple(vec![
                FieldType::Other("TypeA".to_string()),
                FieldType::String,
                FieldType::Other("TypeB".to_string()),
            ]),
            &known,
            &mut acc,
        );

        assert!(acc.contains("TypeA"));
        assert!(acc.contains("TypeB"));
        assert_eq!(acc.len(), 2);
    }

    #[test]
    fn test_collect_refs_struct_type() {
        let known: HashSet<String> = ["FieldType1".to_string()].into_iter().collect();
        let mut acc = HashSet::new();

        collect_refs(
            &FieldType::Struct(vec![
                ("field1".to_string(), FieldType::Other("FieldType1".to_string())),
                ("field2".to_string(), FieldType::I32),
            ]),
            &known,
            &mut acc,
        );

        assert!(acc.contains("FieldType1"));
        assert_eq!(acc.len(), 1);
    }

    #[test]
    fn test_collect_refs_hashmap() {
        let known: HashSet<String> = ["KeyType".to_string(), "ValueType".to_string()]
            .into_iter()
            .collect();
        let mut acc = HashSet::new();

        collect_refs(
            &FieldType::HashMap(
                Box::new(FieldType::Other("KeyType".to_string())),
                Box::new(FieldType::Other("ValueType".to_string())),
            ),
            &known,
            &mut acc,
        );

        assert!(acc.contains("KeyType"));
        assert!(acc.contains("ValueType"));
    }

    #[test]
    fn test_collect_refs_btreemap() {
        let known: HashSet<String> = ["KeyType".to_string()].into_iter().collect();
        let mut acc = HashSet::new();

        collect_refs(
            &FieldType::BTreeMap(
                Box::new(FieldType::Other("KeyType".to_string())),
                Box::new(FieldType::String),
            ),
            &known,
            &mut acc,
        );

        assert!(acc.contains("KeyType"));
        assert_eq!(acc.len(), 1);
    }

    #[test]
    fn test_collect_refs_nested_types() {
        let known: HashSet<String> = ["DeepType".to_string()].into_iter().collect();
        let mut acc = HashSet::new();

        // Option<Vec<DeepType>>
        collect_refs(
            &FieldType::Option(Box::new(FieldType::Vec(Box::new(FieldType::Other(
                "DeepType".to_string(),
            ))))),
            &known,
            &mut acc,
        );

        assert!(acc.contains("DeepType"));
    }

    // ==================== analyse_recursion Tests ====================

    fn create_struct_config(name: &str, fields: Vec<StructField>) -> StructConfig {
        StructConfig {
            struct_name: name.to_string(),
            fields,
            validators: Vec::new(),
        }
    }

    #[test]
    fn test_analyse_recursion_no_types() {
        let structs: HashMap<String, StructConfig> = HashMap::new();
        let enums: HashMap<String, TaggedUnion> = HashMap::new();

        let info = analyse_recursion(&structs, &enums);

        assert!(info.comp_of.is_empty());
        assert!(info.meta.is_empty());
    }

    #[test]
    fn test_analyse_recursion_single_struct_no_deps() {
        let mut structs = HashMap::new();
        structs.insert(
            "User".to_string(),
            create_struct_config(
                "User",
                vec![
                    create_struct_field("name", FieldType::String),
                    create_struct_field("age", FieldType::I32),
                ],
            ),
        );

        let info = analyse_recursion(&structs, &HashMap::new());

        assert!(info.comp_of.contains_key("User"));
        let scc_id = info.comp_of["User"];
        // Single node without self-loop is not recursive
        assert!(!info.meta[&scc_id].0);
    }

    #[test]
    fn test_analyse_recursion_self_referential() {
        let mut structs = HashMap::new();
        structs.insert(
            "Node".to_string(),
            create_struct_config(
                "Node",
                vec![
                    create_struct_field("value", FieldType::I32),
                    create_struct_field(
                        "next",
                        FieldType::Option(Box::new(FieldType::Other("Node".to_string()))),
                    ),
                ],
            ),
        );

        let info = analyse_recursion(&structs, &HashMap::new());

        assert!(info.comp_of.contains_key("Node"));
        // Self-loop should be detected as recursive
        let scc_id = info.comp_of["Node"];
        assert!(info.meta[&scc_id].0);
    }

    #[test]
    fn test_analyse_recursion_mutual_recursion() {
        let mut structs = HashMap::new();
        structs.insert(
            "TypeA".to_string(),
            create_struct_config(
                "TypeA",
                vec![create_struct_field(
                    "b_ref",
                    FieldType::Other("TypeB".to_string()),
                )],
            ),
        );
        structs.insert(
            "TypeB".to_string(),
            create_struct_config(
                "TypeB",
                vec![create_struct_field(
                    "a_ref",
                    FieldType::Other("TypeA".to_string()),
                )],
            ),
        );

        let info = analyse_recursion(&structs, &HashMap::new());

        // Both should be in the same SCC
        assert_eq!(info.comp_of.get("TypeA"), info.comp_of.get("TypeB"));
        let scc_id = info.comp_of["TypeA"];
        assert!(info.meta[&scc_id].0); // Should be recursive
        assert_eq!(info.meta[&scc_id].1.len(), 2); // 2 members in SCC
    }

    #[test]
    fn test_analyse_recursion_chain_no_cycle() {
        let mut structs = HashMap::new();
        structs.insert(
            "A".to_string(),
            create_struct_config(
                "A",
                vec![create_struct_field("b", FieldType::Other("B".to_string()))],
            ),
        );
        structs.insert(
            "B".to_string(),
            create_struct_config(
                "B",
                vec![create_struct_field("c", FieldType::Other("C".to_string()))],
            ),
        );
        structs.insert(
            "C".to_string(),
            create_struct_config("C", vec![create_struct_field("value", FieldType::I32)]),
        );

        let info = analyse_recursion(&structs, &HashMap::new());

        // All should be in different SCCs (no cycles)
        assert_ne!(info.comp_of.get("A"), info.comp_of.get("B"));
        assert_ne!(info.comp_of.get("B"), info.comp_of.get("C"));

        // None should be recursive
        for (_, (is_recursive, _)) in &info.meta {
            assert!(!is_recursive);
        }
    }

    // ==================== deps_of Tests ====================

    #[test]
    fn test_deps_of_no_deps() {
        let mut structs = HashMap::new();
        structs.insert(
            "Simple".to_string(),
            create_struct_config(
                "Simple",
                vec![create_struct_field("value", FieldType::String)],
            ),
        );

        let deps = deps_of("Simple", &structs, &HashMap::new());

        assert!(deps.is_empty());
    }

    #[test]
    fn test_deps_of_with_deps() {
        let mut structs = HashMap::new();
        structs.insert(
            "Parent".to_string(),
            create_struct_config(
                "Parent",
                vec![create_struct_field(
                    "child",
                    FieldType::Other("Child".to_string()),
                )],
            ),
        );
        structs.insert(
            "Child".to_string(),
            create_struct_config("Child", vec![create_struct_field("name", FieldType::String)]),
        );

        let deps = deps_of("Parent", &structs, &HashMap::new());

        assert!(deps.contains("Child"));
        assert_eq!(deps.len(), 1);
    }

    #[test]
    fn test_deps_of_unknown_type() {
        let structs: HashMap<String, StructConfig> = HashMap::new();
        let deps = deps_of("Unknown", &structs, &HashMap::new());

        assert!(deps.is_empty());
    }

    #[test]
    fn test_deps_of_enum_with_variants() {
        let _structs: HashMap<String, StructConfig> = HashMap::new();
        let mut enums: HashMap<String, TaggedUnion> = HashMap::new();

        enums.insert(
            "Status".to_string(),
            TaggedUnion {
                enum_name: "Status".to_string(),
                variants: vec![
                    Variant {
                        name: "Active".to_string(),
                        data: Some(VariantData::DataStructureRef(FieldType::Other(
                            "UserData".to_string(),
                        ))),
                    },
                    Variant {
                        name: "Inactive".to_string(),
                        data: None,
                    },
                ],
            },
        );

        // UserData must be in known set for it to be collected
        let mut structs_with_user = HashMap::new();
        structs_with_user.insert(
            "UserData".to_string(),
            create_struct_config(
                "UserData",
                vec![create_struct_field("name", FieldType::String)],
            ),
        );

        let deps = deps_of("Status", &structs_with_user, &enums);

        assert!(deps.contains("UserData"));
    }

    // ==================== analyse_recursion_tables Tests ====================

    fn create_table_config(name: &str, fields: Vec<StructField>) -> TableConfig {
        TableConfig {
            table_name: name.to_string(),
            struct_config: create_struct_config(name, fields),
            relation: None,
            permissions: None,
            mock_generation_config: None,
            events: Vec::new(),
        }
    }

    #[test]
    fn test_analyse_recursion_tables_empty() {
        let tables: HashMap<String, TableConfig> = HashMap::new();
        let info = analyse_recursion_tables(&tables);

        assert!(info.comp_of.is_empty());
    }

    #[test]
    fn test_analyse_recursion_tables_no_recursion() {
        let mut tables = HashMap::new();
        tables.insert(
            "user".to_string(),
            create_table_config(
                "user",
                vec![create_struct_field("name", FieldType::String)],
            ),
        );

        let info = analyse_recursion_tables(&tables);

        assert!(info.comp_of.contains_key("User"));
    }

    // ==================== deps_of_table Tests ====================

    #[test]
    fn test_deps_of_table_no_deps() {
        let mut tables = HashMap::new();
        tables.insert(
            "user".to_string(),
            create_table_config(
                "user",
                vec![create_struct_field("name", FieldType::String)],
            ),
        );

        let deps = deps_of_table("user", &tables);

        assert!(deps.is_empty());
    }

    #[test]
    fn test_deps_of_table_with_reference() {
        let mut tables = HashMap::new();
        tables.insert(
            "post".to_string(),
            create_table_config(
                "post",
                vec![
                    create_struct_field("title", FieldType::String),
                    create_struct_field("author", FieldType::Other("User".to_string())),
                ],
            ),
        );
        tables.insert(
            "user".to_string(),
            create_table_config(
                "user",
                vec![create_struct_field("name", FieldType::String)],
            ),
        );

        let deps = deps_of_table("post", &tables);

        assert!(deps.contains("user"));
    }

    #[test]
    fn test_deps_of_table_unknown() {
        let tables: HashMap<String, TableConfig> = HashMap::new();
        let deps = deps_of_table("unknown", &tables);

        assert!(deps.is_empty());
    }

    // ==================== sort_tables_by_dependencies Tests ====================
    // These tests are ignored because sort_tables_by_dependencies uses evenframe_log!
    // which requires ABSOLUTE_PATH_TO_EVENFRAME environment variable

    #[test]
    #[ignore = "requires ABSOLUTE_PATH_TO_EVENFRAME environment variable"]
    fn test_sort_tables_empty() {
        let tables: HashMap<String, TableConfig> = HashMap::new();
        let objects: HashMap<String, StructConfig> = HashMap::new();
        let enums: HashMap<String, TaggedUnion> = HashMap::new();

        let sorted = sort_tables_by_dependencies(&tables, &objects, &enums);

        assert!(sorted.is_empty());
    }

    #[test]
    #[ignore = "requires ABSOLUTE_PATH_TO_EVENFRAME environment variable"]
    fn test_sort_tables_no_dependencies() {
        let mut tables = HashMap::new();
        tables.insert(
            "user".to_string(),
            create_table_config(
                "user",
                vec![create_struct_field("name", FieldType::String)],
            ),
        );
        tables.insert(
            "post".to_string(),
            create_table_config(
                "post",
                vec![create_struct_field("title", FieldType::String)],
            ),
        );

        let sorted = sort_tables_by_dependencies(&tables, &HashMap::new(), &HashMap::new());

        assert_eq!(sorted.len(), 2);
        assert!(sorted.contains(&"user".to_string()));
        assert!(sorted.contains(&"post".to_string()));
    }

    #[test]
    #[ignore = "requires ABSOLUTE_PATH_TO_EVENFRAME environment variable"]
    fn test_sort_tables_with_dependency() {
        let mut tables = HashMap::new();
        tables.insert(
            "post".to_string(),
            create_table_config(
                "post",
                vec![
                    create_struct_field("title", FieldType::String),
                    create_struct_field("author", FieldType::Other("User".to_string())),
                ],
            ),
        );
        tables.insert(
            "user".to_string(),
            create_table_config(
                "user",
                vec![create_struct_field("name", FieldType::String)],
            ),
        );

        let sorted = sort_tables_by_dependencies(&tables, &HashMap::new(), &HashMap::new());

        // user should come before post since post depends on user
        let user_pos = sorted.iter().position(|s| s == "user").unwrap();
        let post_pos = sorted.iter().position(|s| s == "post").unwrap();
        assert!(user_pos < post_pos);
    }

    #[test]
    #[ignore = "requires ABSOLUTE_PATH_TO_EVENFRAME environment variable"]
    fn test_sort_tables_chain_dependency() {
        let mut tables = HashMap::new();
        tables.insert(
            "C".to_string(),
            create_table_config(
                "C",
                vec![create_struct_field("b_ref", FieldType::Other("B".to_string()))],
            ),
        );
        tables.insert(
            "B".to_string(),
            create_table_config(
                "B",
                vec![create_struct_field("a_ref", FieldType::Other("A".to_string()))],
            ),
        );
        tables.insert(
            "A".to_string(),
            create_table_config("A", vec![create_struct_field("value", FieldType::I32)]),
        );

        let sorted = sort_tables_by_dependencies(&tables, &HashMap::new(), &HashMap::new());

        // A should come first, then B, then C
        let a_pos = sorted.iter().position(|s| s == "A").unwrap();
        let b_pos = sorted.iter().position(|s| s == "B").unwrap();
        let c_pos = sorted.iter().position(|s| s == "C").unwrap();
        assert!(a_pos < b_pos);
        assert!(b_pos < c_pos);
    }

    #[test]
    #[ignore = "requires ABSOLUTE_PATH_TO_EVENFRAME environment variable"]
    fn test_sort_tables_circular_dependency() {
        let mut tables = HashMap::new();
        tables.insert(
            "A".to_string(),
            create_table_config(
                "A",
                vec![create_struct_field("b_ref", FieldType::Other("B".to_string()))],
            ),
        );
        tables.insert(
            "B".to_string(),
            create_table_config(
                "B",
                vec![create_struct_field("a_ref", FieldType::Other("A".to_string()))],
            ),
        );

        let sorted = sort_tables_by_dependencies(&tables, &HashMap::new(), &HashMap::new());

        // Both should be in the result (circular deps are handled via SCC)
        assert_eq!(sorted.len(), 2);
        assert!(sorted.contains(&"A".to_string()));
        assert!(sorted.contains(&"B".to_string()));
    }

    // ==================== collect_field_type_dependencies Tests ====================

    #[test]
    fn test_collect_field_type_dependencies_primitive() {
        let tables: HashMap<String, TableConfig> = HashMap::new();
        let objects: HashMap<String, StructConfig> = HashMap::new();
        let enums: HashMap<String, TaggedUnion> = HashMap::new();
        let mut deps = HashSet::new();
        let mut visited = HashSet::new();

        collect_field_type_dependencies(
            &FieldType::String,
            &tables,
            &objects,
            &enums,
            &mut deps,
            &mut visited,
        );

        assert!(deps.is_empty());
    }

    #[test]
    fn test_collect_field_type_dependencies_table_ref() {
        let mut tables = HashMap::new();
        tables.insert(
            "user".to_string(),
            create_table_config(
                "user",
                vec![create_struct_field("name", FieldType::String)],
            ),
        );

        let mut deps = HashSet::new();
        let mut visited = HashSet::new();

        collect_field_type_dependencies(
            &FieldType::Other("user".to_string()),
            &tables,
            &HashMap::new(),
            &HashMap::new(),
            &mut deps,
            &mut visited,
        );

        assert!(deps.contains("user"));
    }

    #[test]
    fn test_collect_field_type_dependencies_avoids_infinite_recursion() {
        let mut tables = HashMap::new();
        tables.insert(
            "node".to_string(),
            create_table_config(
                "node",
                vec![create_struct_field(
                    "child",
                    FieldType::Option(Box::new(FieldType::Other("node".to_string()))),
                )],
            ),
        );

        let mut deps = HashSet::new();
        let mut visited = HashSet::new();

        collect_field_type_dependencies(
            &FieldType::Other("node".to_string()),
            &tables,
            &HashMap::new(),
            &HashMap::new(),
            &mut deps,
            &mut visited,
        );

        // Should not hang due to infinite recursion
        assert!(visited.contains("node"));
    }

    #[test]
    fn test_collect_field_type_dependencies_nested_option() {
        let mut tables = HashMap::new();
        tables.insert(
            "item".to_string(),
            create_table_config(
                "item",
                vec![create_struct_field("name", FieldType::String)],
            ),
        );

        let mut deps = HashSet::new();
        let mut visited = HashSet::new();

        collect_field_type_dependencies(
            &FieldType::Option(Box::new(FieldType::Other("item".to_string()))),
            &tables,
            &HashMap::new(),
            &HashMap::new(),
            &mut deps,
            &mut visited,
        );

        assert!(deps.contains("item"));
    }

    #[test]
    fn test_collect_field_type_dependencies_vec() {
        let mut tables = HashMap::new();
        tables.insert(
            "item".to_string(),
            create_table_config(
                "item",
                vec![create_struct_field("name", FieldType::String)],
            ),
        );

        let mut deps = HashSet::new();
        let mut visited = HashSet::new();

        collect_field_type_dependencies(
            &FieldType::Vec(Box::new(FieldType::Other("item".to_string()))),
            &tables,
            &HashMap::new(),
            &HashMap::new(),
            &mut deps,
            &mut visited,
        );

        assert!(deps.contains("item"));
    }

    #[test]
    fn test_collect_field_type_dependencies_tuple() {
        let mut tables = HashMap::new();
        tables.insert(
            "a".to_string(),
            create_table_config(
                "a",
                vec![create_struct_field("val", FieldType::String)],
            ),
        );
        tables.insert(
            "b".to_string(),
            create_table_config(
                "b",
                vec![create_struct_field("val", FieldType::I32)],
            ),
        );

        let mut deps = HashSet::new();
        let mut visited = HashSet::new();

        collect_field_type_dependencies(
            &FieldType::Tuple(vec![
                FieldType::Other("a".to_string()),
                FieldType::Other("b".to_string()),
            ]),
            &tables,
            &HashMap::new(),
            &HashMap::new(),
            &mut deps,
            &mut visited,
        );

        assert!(deps.contains("a"));
        assert!(deps.contains("b"));
    }

    #[test]
    fn test_collect_field_type_dependencies_hashmap_values() {
        let mut tables = HashMap::new();
        tables.insert(
            "value_type".to_string(),
            create_table_config(
                "value_type",
                vec![create_struct_field("data", FieldType::String)],
            ),
        );

        let mut deps = HashSet::new();
        let mut visited = HashSet::new();

        collect_field_type_dependencies(
            &FieldType::HashMap(
                Box::new(FieldType::String),
                Box::new(FieldType::Other("value_type".to_string())),
            ),
            &tables,
            &HashMap::new(),
            &HashMap::new(),
            &mut deps,
            &mut visited,
        );

        assert!(deps.contains("value_type"));
    }

    #[test]
    fn test_collect_field_type_dependencies_with_objects() {
        let mut objects = HashMap::new();
        objects.insert(
            "Address".to_string(),
            create_struct_config(
                "Address",
                vec![
                    create_struct_field("street", FieldType::String),
                    create_struct_field("city", FieldType::Other("City".to_string())),
                ],
            ),
        );

        let mut tables = HashMap::new();
        tables.insert(
            "City".to_string(),
            create_table_config(
                "City",
                vec![create_struct_field("name", FieldType::String)],
            ),
        );

        let mut deps = HashSet::new();
        let mut visited = HashSet::new();

        collect_field_type_dependencies(
            &FieldType::Other("Address".to_string()),
            &tables,
            &objects,
            &HashMap::new(),
            &mut deps,
            &mut visited,
        );

        // Should find City through Address's fields
        assert!(deps.contains("City"));
    }

    #[test]
    fn test_collect_field_type_dependencies_with_enums() {
        let mut enums = HashMap::new();
        enums.insert(
            "Status".to_string(),
            TaggedUnion {
                enum_name: "Status".to_string(),
                variants: vec![Variant {
                    name: "WithData".to_string(),
                    data: Some(VariantData::DataStructureRef(FieldType::Other(
                        "data_table".to_string(),
                    ))),
                }],
            },
        );

        let mut tables = HashMap::new();
        tables.insert(
            "data_table".to_string(),
            create_table_config(
                "data_table",
                vec![create_struct_field("value", FieldType::I32)],
            ),
        );

        let mut deps = HashSet::new();
        let mut visited = HashSet::new();

        collect_field_type_dependencies(
            &FieldType::Other("Status".to_string()),
            &tables,
            &HashMap::new(),
            &enums,
            &mut deps,
            &mut visited,
        );

        assert!(deps.contains("data_table"));
    }

    #[test]
    fn test_collect_field_type_dependencies_record_link() {
        let mut tables = HashMap::new();
        tables.insert(
            "linked".to_string(),
            create_table_config(
                "linked",
                vec![create_struct_field("name", FieldType::String)],
            ),
        );

        let mut deps = HashSet::new();
        let mut visited = HashSet::new();

        collect_field_type_dependencies(
            &FieldType::RecordLink(Box::new(FieldType::Other("linked".to_string()))),
            &tables,
            &HashMap::new(),
            &HashMap::new(),
            &mut deps,
            &mut visited,
        );

        assert!(deps.contains("linked"));
    }

    #[test]
    fn test_collect_field_type_dependencies_struct_fields() {
        let mut tables = HashMap::new();
        tables.insert(
            "inner".to_string(),
            create_table_config(
                "inner",
                vec![create_struct_field("data", FieldType::String)],
            ),
        );

        let mut deps = HashSet::new();
        let mut visited = HashSet::new();

        collect_field_type_dependencies(
            &FieldType::Struct(vec![
                ("field1".to_string(), FieldType::String),
                ("field2".to_string(), FieldType::Other("inner".to_string())),
            ]),
            &tables,
            &HashMap::new(),
            &HashMap::new(),
            &mut deps,
            &mut visited,
        );

        assert!(deps.contains("inner"));
    }

    #[test]
    fn test_collect_field_type_dependencies_btreemap() {
        let mut tables = HashMap::new();
        tables.insert(
            "key_type".to_string(),
            create_table_config(
                "key_type",
                vec![create_struct_field("id", FieldType::String)],
            ),
        );

        let mut deps = HashSet::new();
        let mut visited = HashSet::new();

        collect_field_type_dependencies(
            &FieldType::BTreeMap(
                Box::new(FieldType::Other("key_type".to_string())),
                Box::new(FieldType::I32),
            ),
            &tables,
            &HashMap::new(),
            &HashMap::new(),
            &mut deps,
            &mut visited,
        );

        assert!(deps.contains("key_type"));
    }
}
