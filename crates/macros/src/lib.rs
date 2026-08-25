use proc_macro::TokenStream;
use quote::quote;

#[cfg(feature = "migrate")]
#[proc_macro]
pub fn embed_migrations(input: TokenStream) -> TokenStream {
    let dir = syn::parse_macro_input!(input as syn::LitStr);

    match migrations::embed(&dir.value()) {
        Ok(tokens) => tokens.into(),
        Err(message) => syn::Error::new(dir.span(), message)
            .to_compile_error()
            .into(),
    }
}

#[cfg(feature = "migrate")]
mod migrations {
    use esql_migrate::Migration;
    use proc_macro2::TokenStream;
    use quote::quote;
    use std::{env, fmt::Display, fs::read_dir, path::Path};

    pub(super) fn embed(dir: &str) -> Result<TokenStream, String> {
        let root = env::var("CARGO_MANIFEST_DIR").map_err(reason("no CARGO_MANIFEST_DIR"))?;
        let path = Path::new(&root).join(dir);

        let mut files = read_dir(&path)
            .map_err(reason("cannot read migration directory"))?
            .map(|entry| {
                Ok(entry
                    .map_err(reason("cannot read migration directory"))?
                    .path())
            })
            .collect::<Result<Vec<_>, String>>()?;

        files.sort();

        let mut migrations = Vec::with_capacity(files.len());

        for file in &files {
            migrations
                .push(Migration::new(file).map_err(|e| format!("{}: {}", e.filename, e.error))?);
        }

        migrations.sort_by_key(|m| m.version);

        let migrations = migrations.into_iter().map(|migration| {
            let Migration {
                checksum,
                name,
                sql,
                version,
            } = migration;

            quote! {
                esql::migrate::Migration {
                    checksum: #checksum.to_string(),
                    name: #name.to_string(),
                    sql: #sql.to_string(),
                    version: #version,
                }
            }
        });

        // Cargo learns which files this expansion depends on from the paths
        // that reach the compiler, so without these a migration can be edited
        // without the binary being rebuilt.
        let tracked = files.iter().map(|file| {
            let file = file.to_string_lossy();
            quote! { const _: &str = include_str!(#file); }
        });

        Ok(quote! {{
            #(#tracked)*
            esql::migrate::Migrator::new(vec![ #(#migrations),* ])
        }})
    }

    fn reason<E: Display>(what: &'static str) -> impl Fn(E) -> String {
        move |error| format!("{what}: {error}")
    }
}

#[cfg(feature = "derive")]
#[proc_macro_derive(FromRow)]
pub fn derive_from_row(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let fields = match &input.data {
        syn::Data::Struct(data) => match &data.fields {
            syn::Fields::Named(fields) => &fields.named,
            _ => panic!("FromRow can only be derived for structs with named fields"),
        },
        _ => panic!("FromRow can only be derived for structs"),
    };

    let field_extractions = fields.iter().map(|f| {
        let ident = f.ident.as_ref().unwrap();
        let column = ident.to_string();
        quote! { #ident: row.try_get(#column)? }
    });

    quote! {
        impl #impl_generics esql::FromRow for #name #ty_generics #where_clause {
            fn from_row<R: esql::Row>(row: &R) -> Result<Self, esql::FromRowError> {
                Ok(Self {
                    #(#field_extractions),*
                })
            }
        }
    }
    .into()
}
