use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DataStruct, DeriveInput, Fields};

use crate::misc::add_traits_bounds;

/// `Builder Pattern` derive macro implementation.
pub fn impl_builder(input: DeriveInput) -> TokenStream {
    // Destructures input data and extracts fields of struct.
    let Data::Struct(DataStruct { fields: Fields::Named(fields), .. }) = input.data else {
        let err_msg = "`#[derive(Builder)]` is only supported for structs with named fields";
        return syn::Error::new(input.ident.span(), err_msg).into_compile_error();
    };

    let (name, vis) = (input.ident, input.vis);
    let generics = add_traits_bounds(input.generics, [quote!(Default), quote!(Clone)]);
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let builder_name = format_ident!("{name}Builder");
    let builder_doc = format!("Builder for [`{name}`].");

    // Generates setter code for all fields.
    let setters = fields.named.into_iter().map(|f| {
        let (field, ty) = (f.ident.unwrap(), f.ty);
        let doc = format!("Set [`{name}::{field}`].");
        quote! {
            #[doc = #doc]
            #[inline]
            #vis fn #field(&mut self, #field: #ty) -> &mut Self {
                self.0.#field = #field;
                self
            }
        }
    });

    quote! {
        #[doc = #builder_doc]
        #vis struct #builder_name #impl_generics(#name #ty_generics) #where_clause;

        impl #impl_generics #name #ty_generics #where_clause {
            /// Returns a new builder.
            #[inline]
            #vis fn builder() -> #builder_name #ty_generics {
                #builder_name::new()
            }
        }

        impl #impl_generics #builder_name #ty_generics #where_clause {
            /// Constructs a new `Builder`.
            #[inline]
            #vis fn new() -> Self {
                Self(Default::default())
            }

            /// Invokes the builder and returns the object.
            #[inline]
            #vis fn build(&self) -> #name #ty_generics {
                Clone::clone(&self.0)
            }

            #(#setters)*
        }
    }
}
