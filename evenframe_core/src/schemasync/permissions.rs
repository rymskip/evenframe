use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use syn::parenthesized;
use tracing::{debug, info, trace, warn};

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PermissionsConfig {
    pub all_permissions: Option<String>,
    pub select_permissions: Option<String>,
    pub update_permissions: Option<String>,
    pub delete_permissions: Option<String>,
    pub create_permissions: Option<String>,
}

impl ToTokens for PermissionsConfig {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let all = if let Some(ref s) = self.all_permissions {
            quote! { Some(#s.to_string()) }
        } else {
            quote! { None }
        };
        let select = if let Some(ref s) = self.select_permissions {
            quote! { Some(#s.to_string()) }
        } else {
            quote! { None }
        };
        let update = if let Some(ref s) = self.update_permissions {
            quote! { Some(#s.to_string()) }
        } else {
            quote! { None }
        };
        let delete = if let Some(ref s) = self.delete_permissions {
            quote! { Some(#s.to_string()) }
        } else {
            quote! { None }
        };
        let create = if let Some(ref s) = self.create_permissions {
            quote! { Some(#s.to_string()) }
        } else {
            quote! { None }
        };

        tokens.extend(quote! {
            ::evenframe::schemasync::PermissionsConfig {
                all_permissions: #all,
                select_permissions: #select,
                update_permissions: #update,
                delete_permissions: #delete,
                create_permissions: #create,
            }
        });
    }
}

impl PermissionsConfig {
    /// A `#[permissions(...)]` attribute as it is written, for the messages
    /// that show one.
    pub const EXAMPLE: &str = "#[permissions(\n    select(\"true\"),\n    create(\"$auth.role = 'admin'\"),\n    update(\"$auth.id = id\"),\n    delete(\"false\")\n)]";

    /// The condition for `select`: its own, else `all`'s, else `FULL`.
    pub fn select(&self) -> &str {
        self.condition(&self.select_permissions)
    }

    /// The condition for `create`: its own, else `all`'s, else `FULL`.
    pub fn create(&self) -> &str {
        self.condition(&self.create_permissions)
    }

    /// The condition for `update`: its own, else `all`'s, else `FULL`.
    pub fn update(&self) -> &str {
        self.condition(&self.update_permissions)
    }

    /// The condition for `delete`: its own, else `all`'s, else `FULL`.
    pub fn delete(&self) -> &str {
        self.condition(&self.delete_permissions)
    }

    fn condition<'a>(&'a self, own: &'a Option<String>) -> &'a str {
        own.as_deref()
            .or(self.all_permissions.as_deref())
            .unwrap_or("FULL")
    }

    pub fn parse(attrs: &[syn::Attribute]) -> syn::Result<Option<PermissionsConfig>> {
        debug!(
            "Parsing permissions configuration from {} attributes",
            attrs.len()
        );
        let mut all_permissions: Option<String> = None;
        let mut select_permissions: Option<String> = None;
        let mut update_permissions: Option<String> = None;
        let mut delete_permissions: Option<String> = None;
        let mut create_permissions: Option<String> = None;

        for (i, attr) in attrs.iter().enumerate() {
            trace!("Processing attribute {} of {}", i + 1, attrs.len());
            if attr.path().is_ident("permissions") {
                debug!("Found permissions attribute");
                attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("all") {
                        trace!("Parsing all permissions attribute");
                        let content;
                        parenthesized!(content in meta.input);
                        if all_permissions.is_some() {
                            warn!("Duplicate all permissions attribute found");
                            return Err(meta.error("duplicate all permissions attribute"));
                        }
                        let permission = content.parse::<syn::LitStr>()?.value();
                        trace!("Parsed all permissions: {}", permission);
                        all_permissions = Some(permission);
                        return Ok(());
                    }
                    if meta.path.is_ident("select") {
                        trace!("Parsing select permissions attribute");
                        let content;
                        parenthesized!(content in meta.input);
                        if select_permissions.is_some() {
                            warn!("Duplicate select permissions attribute found");
                            return Err(meta.error("duplicate select permissions attribute"));
                        }
                        let permission = content.parse::<syn::LitStr>()?.value();
                        trace!("Parsed select permissions: {}", permission);
                        select_permissions = Some(permission);
                        return Ok(());
                    }
                    if meta.path.is_ident("update") {
                        trace!("Parsing update permissions attribute");
                        let content;
                        parenthesized!(content in meta.input);
                        if update_permissions.is_some() {
                            warn!("Duplicate update permissions attribute found");
                            return Err(meta.error("duplicate update permissions attribute"));
                        }
                        let permission = content.parse::<syn::LitStr>()?.value();
                        trace!("Parsed update permissions: {}", permission);
                        update_permissions = Some(permission);
                        return Ok(());
                    }
                    if meta.path.is_ident("delete") {
                        trace!("Parsing delete permissions attribute");
                        let content;
                        parenthesized!(content in meta.input);
                        if delete_permissions.is_some() {
                            warn!("Duplicate delete permissions attribute found");
                            return Err(meta.error("duplicate delete permissions attribute"));
                        }
                        let permission = content.parse::<syn::LitStr>()?.value();
                        trace!("Parsed delete permissions: {}", permission);
                        delete_permissions = Some(permission);
                        return Ok(());
                    }
                    if meta.path.is_ident("create") {
                        trace!("Parsing create permissions attribute");
                        let content;
                        parenthesized!(content in meta.input);
                        if create_permissions.is_some() {
                            warn!("Duplicate create permissions attribute found");
                            return Err(meta.error("duplicate create permissions attribute"));
                        }
                        let permission = content.parse::<syn::LitStr>()?.value();
                        trace!("Parsed create permissions: {}", permission);
                        create_permissions = Some(permission);
                        return Ok(());
                    }

                    let path = meta.path.to_token_stream().to_string();
                    warn!("Unrecognized permission type: {}", path);
                    Err(meta.error("unrecognized permission type"))
                })?;

                let permissions_config = PermissionsConfig {
                    all_permissions: all_permissions.clone(),
                    select_permissions: select_permissions.clone(),
                    update_permissions: update_permissions.clone(),
                    delete_permissions: delete_permissions.clone(),
                    create_permissions: create_permissions.clone(),
                };

                info!("Successfully parsed permissions configuration");
                debug!(
                    "Permission details - all: {}, select: {}, update: {}, delete: {}, create: {}",
                    all_permissions.is_some(),
                    select_permissions.is_some(),
                    update_permissions.is_some(),
                    delete_permissions.is_some(),
                    create_permissions.is_some()
                );

                return Ok(Some(permissions_config));
            }
        }

        debug!("No permissions attribute found");
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::PermissionsConfig;

    fn permissions(source: &str) -> PermissionsConfig {
        let item: syn::DeriveInput =
            syn::parse_str(&format!("{source} struct Item;")).expect("the attribute parses");
        PermissionsConfig::parse(&item.attrs)
            .expect("the permissions are valid")
            .expect("the item has permissions")
    }

    #[test]
    fn the_example_is_an_attribute_the_parser_reads() {
        let parsed = permissions(PermissionsConfig::EXAMPLE);
        assert_eq!(parsed.select(), "true");
        assert_eq!(parsed.create(), "$auth.role = 'admin'");
        assert_eq!(parsed.update(), "$auth.id = id");
        assert_eq!(parsed.delete(), "false");
    }

    #[test]
    fn all_covers_each_action_without_a_condition_of_its_own() {
        let parsed = permissions(r#"#[permissions(all("false"), select("true"))]"#);
        assert_eq!(parsed.select(), "true");
        assert_eq!(parsed.create(), "false");
        assert_eq!(parsed.update(), "false");
        assert_eq!(parsed.delete(), "false");
    }

    #[test]
    fn an_action_left_out_is_full() {
        let parsed = permissions(r#"#[permissions(delete("false"))]"#);
        assert_eq!(parsed.select(), "FULL");
        assert_eq!(parsed.delete(), "false");
    }

    #[test]
    fn the_assignment_form_is_refused() {
        let item: syn::DeriveInput =
            syn::parse_str(r#"#[permissions(select = "true")] struct Item;"#)
                .expect("the attribute parses");
        assert!(PermissionsConfig::parse(&item.attrs).is_err());
    }
}
