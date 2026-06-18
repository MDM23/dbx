use proc_macro::TokenStream;
use quote::quote;

#[cfg(feature = "migrate")]
#[proc_macro]
pub fn embed_migrations(input: TokenStream) -> TokenStream {
    use esql_migrate::Migration;
    use std::{env, fs::read_dir, path::Path};
    use syn::LitStr;

    let dir = syn::parse_macro_input!(input as LitStr);
    let path = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join(&dir.value());

    let mut migrations: Vec<Migration> = read_dir(path)
        .unwrap()
        .map(|e| {
            e.map(|e| Migration::try_from(e.path().as_path()).unwrap())
                .unwrap()
        })
        .collect();

    migrations.sort_by_key(|m| m.version);

    let migrations = migrations.into_iter().map(|m| {
        let Migration {
            checksum,
            name,
            sql,
            version,
        } = m;

        quote! {
            esql::migrate::Migration {
                checksum: #checksum.to_string(),
                name: #name.to_string(),
                sql: #sql.to_string(),
                version: #version,
            }
        }
    });

    quote! {
        esql::migrate::Migrator::new(
            vec![ #(#migrations),* ]
        )
    }
    .into()
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
