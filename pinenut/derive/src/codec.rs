use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use syn::{
    spanned::Spanned, Data, DataStruct, DeriveInput, Fields, GenericParam, Generics, Lifetime,
    LifetimeParam,
};

use crate::misc::add_traits_bounds;

/// `Encode` derive macro implementation.
pub fn impl_encode(input: DeriveInput) -> TokenStream {
    // Destructures input data and extracts fields of struct.
    let Data::Struct(DataStruct { fields, .. }) = input.data else {
        let err_msg = "`#[derive(Encode)]` is only supported for structs";
        return syn::Error::new(input.ident.span(), err_msg).into_compile_error();
    };

    let name = input.ident;
    let generics = add_traits_bounds(input.generics, [quote!(crate::codec::Encode)]);
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let body = fields.into_iter().enumerate().map(|(i, f)| {
        let name = f.ident.map(|f| f.into_token_stream()).unwrap_or(i.into_token_stream());
        quote! { crate::codec::Encode::encode(&self.#name, sink)?; }
    });

    quote! {
        impl #impl_generics crate::codec::Encode for #name #ty_generics #where_clause {
            fn encode<S>(&self, sink: &mut S) -> Result<(), S::Error>
            where
                S: crate::codec::Sink,
            {
                #(#body)*
                Ok(())
            }
        }
    }
}

/// `Decode` derive macro implementation.
pub fn impl_decode(input: DeriveInput) -> TokenStream {
    // Destructures input data and extracts fields of struct.
    let Data::Struct(DataStruct { fields, .. }) = input.data else {
        let err_msg = "`#[derive(Decode)]` is only supported for structs";
        return syn::Error::new(input.ident.span(), err_msg).into_compile_error();
    };

    let name = input.ident;
    let generics = add_traits_bounds(input.generics, [quote!(crate::codec::Decode<'de>)]);
    let (_, ty_generics, where_clause) = generics.split_for_impl();
    let generics = add_lifetime_bounds(generics.clone());
    let impl_generics = generics.split_for_impl().0;

    let construct_body = match fields {
        Fields::Named(fields) => {
            let decode_fields = fields.named.into_iter().map(|f| {
                let name = f.ident.unwrap();
                quote! { #name: crate::codec::Decode::decode(source)?, }
            });
            quote! { { #(#decode_fields)* } }
        }
        Fields::Unnamed(fields) => {
            let decode_fields = fields.unnamed.into_iter().map(|_| {
                quote! { crate::codec::Decode::decode(source)?, }
            });
            quote! { ( #(#decode_fields)* ) }
        }
        Fields::Unit => quote!(),
    };

    quote! {
        impl #impl_generics crate::codec::Decode<'de> for #name #ty_generics #where_clause {
            fn decode<S>(source: &mut S) -> Result<Self, S::Error>
            where
                S: crate::codec::Source<'de>,
            {
                Ok(#name #construct_body)
            }
        }
    }
}

/// Add the lifetime bounds with every lifetimes to `'de` : `'de: 'a + 'b + 'c`.
fn add_lifetime_bounds(mut generics: Generics) -> Generics {
    let lifetimes = generics.params.iter().filter_map(|p| {
        if let GenericParam::Lifetime(lifetime) = p {
            Some(lifetime.lifetime.clone())
        } else {
            None
        }
    });

    let mut de_lifetime = LifetimeParam::new(Lifetime::new("'de", generics.span()));
    de_lifetime.bounds.extend(lifetimes);

    generics.params.push(GenericParam::Lifetime(de_lifetime));
    generics
}
