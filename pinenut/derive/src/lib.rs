//! The internal derive crate for `Pinenut`. Implements the following derive
//! macros: `pinenut_derive::Builder`, `pinenut_derive::Encode`,
//! `pinenut_derive::Decode`.

use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

use crate::{
    builder::impl_builder,
    codec::{impl_decode, impl_encode},
};

mod builder;
mod codec;
mod misc;

/// `Derive Macro` to automatically implement the `Builder Pattern` for structs that
/// already implement the `Default` and `Clone` traits.
///
/// # Examples
///
/// Use it as follows:
///
/// ```
/// use pinenut_derive::Builder;
///
/// #[derive(Builder, Default, Clone)]
/// pub struct MyStruct<T> {
///     field_one: i32,
///     field_two: String,
///     field_three: T,
/// }
/// ```
///
/// Will generate code looks like this:
///
/// ```
/// # #[derive(Default, Clone)]
/// # pub struct MyStruct<T> {
/// #     field_one: i32,
/// #     field_two: String,
/// #     field_three: T,
/// # }
/// pub struct MyStructBuilder<T: Default + Clone>(MyStruct<T>);
///
/// impl<T: Default + Clone> MyStruct<T> {
///     pub fn builder() -> MyStructBuilder<T> {
///         MyStructBuilder::new()
///     }
/// }
///
/// impl<T: Default + Clone> MyStructBuilder<T> {
///     pub fn new() -> Self {
///         Self(Default::default())
///     }
///
///     pub fn build(&self) -> MyStruct<T> {
///         Clone::clone(&self.0)
///     }
///
///     pub fn field_one(&mut self, field_one: i32) -> &mut Self {
///         self.0.field_one = field_one;
///         self
///     }
///
///     pub fn field_two(&mut self, field_two: String) -> &mut Self {
///         self.0.field_two = field_two;
///         self
///     }
///
///     pub fn field_three(&mut self, field_three: T) -> &mut Self {
///         self.0.field_three = field_three;
///         self
///     }
/// }
/// ```
#[proc_macro_derive(Builder)]
pub fn derive_builder(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    impl_builder(input).into()
}

/// `Derive Macro` to automatically implement the `Encode` trait for structs.
///
/// # Examples
///
/// Use it as follows:
///
/// ```ignore
/// use pinenut_derive::Encode;
///
/// #[derive(Encode)]
/// struct MyStruct<'a, T> {
///     field_one: T,
///     field_two: &'a str,
/// }
/// ```
///
/// Will generate code looks like this:
///
/// ```ignore
/// impl<'a, T: codec::Encode> codec::Encode for MyStruct<'a, T> {
///     fn encode<S>(&self, sink: &mut S) -> Result<(), S::Error>
///         where
///             S: codec::Sink,
///     {
///         codec::Encode::encode(&self.field_one, sink)?;
///         codec::Encode::encode(&self.field_two, sink)?;
///         Ok(())
///     }
/// }
/// ```
#[proc_macro_derive(Encode)]
pub fn derive_encode(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    impl_encode(input).into()
}

/// `Derive Macro` to automatically implement the `Decode` trait for structs.
///
/// # Examples
///
/// Use it as follows:
///
/// ```ignore
/// use pinenut_derive::Encode;
///
/// #[derive(Decode)]
/// struct MyStruct<'a, T> {
///     field_one: T,
///     field_two: &'a str,
/// }
/// ```
///
/// Will generate code looks like this:
///
/// ```ignore
/// impl<'de: 'a, 'a, T: codec::Decode<'de>> codec::Decode<'de> for MyStruct<'a, T> {
///     fn decode<S>(source: &mut S) -> Result<Self, S::Error>
///         where
///             S: codec::Source<'de>,
///     {
///         Ok(MyStruct {
///             field_one: codec::Decode::decode(source)?,
///             field_two: codec::Decode::decode(source)?,
///         })
///     }
/// }
/// ```
#[proc_macro_derive(Decode)]
pub fn derive_decode(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    impl_decode(input).into()
}
