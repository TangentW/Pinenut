use proc_macro2::TokenStream;
use syn::{parse_quote, GenericParam, Generics};

/// Add the trait bounds to every type parameter T: `T: A + B + C`.
pub fn add_traits_bounds(
    mut generics: Generics,
    trait_bounds: impl IntoIterator<Item = TokenStream>,
) -> Generics {
    for trait_bound in trait_bounds.into_iter() {
        for param in &mut generics.params {
            if let GenericParam::Type(ty_param) = param {
                ty_param.bounds.push(parse_quote! { #trait_bound })
            }
        }
    }
    generics
}
