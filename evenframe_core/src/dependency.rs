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
