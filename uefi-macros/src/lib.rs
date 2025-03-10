#![recursion_limit = "128"]

extern crate proc_macro;

use proc_macro::TokenStream;

use proc_macro2::{TokenStream as TokenStream2, TokenTree};
use quote::{quote, ToTokens, TokenStreamExt};
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    spanned::Spanned,
    DeriveInput, Error, Generics, Ident, ItemFn, ItemType, LitStr, Visibility,
};

/// Parses a type definition, extracts its identifier and generic parameters
struct TypeDefinition {
    ident: Ident,
    generics: Generics,
}

impl Parse for TypeDefinition {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if let Ok(d) = DeriveInput::parse(input) {
            Ok(Self {
                ident: d.ident,
                generics: d.generics,
            })
        } else if let Ok(t) = ItemType::parse(input) {
            Ok(Self {
                ident: t.ident,
                generics: t.generics,
            })
        } else {
            Err(input.error("Input is not an alias, enum, struct or union definition"))
        }
    }
}

macro_rules! err {
    ($span:expr, $message:expr $(,)?) => {
        Error::new($span.span(), $message).to_compile_error()
    };
    ($span:expr, $message:expr, $($args:expr),*) => {
        Error::new($span.span(), format!($message, $($args),*)).to_compile_error()
    };
}

/// `unsafe_guid` attribute macro, implements the `Identify` trait for any type
/// (mostly works like a custom derive, but also supports type aliases)
#[proc_macro_attribute]
pub fn unsafe_guid(args: TokenStream, input: TokenStream) -> TokenStream {
    // Parse the arguments and input using Syn
    let (time_low, time_mid, time_high_and_version, clock_seq_and_variant, node) =
        match parse_guid(parse_macro_input!(args as LitStr)) {
            Ok(data) => data,
            Err(tokens) => return tokens.into(),
        };

    let mut result: TokenStream2 = input.clone().into();

    let type_definition = parse_macro_input!(input as TypeDefinition);

    // At this point, we know everything we need to implement Identify
    let ident = &type_definition.ident;
    let (impl_generics, ty_generics, where_clause) = type_definition.generics.split_for_impl();

    result.append_all(quote! {
        unsafe impl #impl_generics ::uefi::Identify for #ident #ty_generics #where_clause {
            #[doc(hidden)]
            #[allow(clippy::unreadable_literal)]
            const GUID: ::uefi::Guid = ::uefi::Guid::from_values(
                #time_low,
                #time_mid,
                #time_high_and_version,
                #clock_seq_and_variant,
                #node,
            );
        }
    });
    result.into()
}

fn parse_guid(guid_lit: LitStr) -> Result<(u32, u16, u16, u16, u64), TokenStream2> {
    let guid_str = guid_lit.value();

    // We expect a canonical GUID string, such as "12345678-9abc-def0-fedc-ba9876543210"
    if guid_str.len() != 36 {
        return Err(err!(
            guid_lit,
            "\"{}\" is not a canonical GUID string (expected 36 bytes, found {})",
            guid_str,
            guid_str.len()
        ));
    }
    let mut offset = 1; // 1 is for the starting quote
    let mut guid_hex_iter = guid_str.split('-');
    let mut next_guid_int = |len: usize| -> Result<u64, TokenStream2> {
        let guid_hex_component = guid_hex_iter.next().unwrap();

        // convert syn::LitStr to proc_macro2::Literal..
        let lit = match guid_lit.to_token_stream().into_iter().next().unwrap() {
            TokenTree::Literal(lit) => lit,
            _ => unreachable!(),
        };
        // ..so that we can call subspan and nightly users (us) will get the fancy span
        let span = lit
            .subspan(offset..offset + guid_hex_component.len())
            .unwrap_or_else(|| lit.span());

        if guid_hex_component.len() != len * 2 {
            return Err(err!(
                span,
                "GUID component \"{}\" is not a {}-bit hexadecimal string",
                guid_hex_component,
                len * 8
            ));
        }
        offset += guid_hex_component.len() + 1; // + 1 for the dash
        u64::from_str_radix(guid_hex_component, 16).map_err(|_| {
            err!(
                span,
                "GUID component \"{}\" is not a hexadecimal number",
                guid_hex_component
            )
        })
    };

    // The GUID string is composed of a 32-bit integer, three 16-bit ones, and a 48-bit one
    Ok((
        next_guid_int(4)? as u32,
        next_guid_int(2)? as u16,
        next_guid_int(2)? as u16,
        next_guid_int(2)? as u16,
        next_guid_int(6)?,
    ))
}

/// Custom derive for the `Protocol` trait
#[proc_macro_derive(Protocol)]
pub fn derive_protocol(item: TokenStream) -> TokenStream {
    // Parse the input using Syn
    let item = parse_macro_input!(item as DeriveInput);

    // Then implement Protocol
    let ident = item.ident.clone();
    let (impl_generics, ty_generics, where_clause) = item.generics.split_for_impl();
    let result = quote! {
        // Mark this as a `Protocol` implementation
        impl #impl_generics ::uefi::proto::Protocol for #ident #ty_generics #where_clause {}

        // Most UEFI functions expect to be called on the bootstrap processor.
        impl #impl_generics !Send for #ident #ty_generics #where_clause {}

        // Most UEFI functions do not support multithreaded access.
        impl #impl_generics !Sync for #ident #ty_generics #where_clause {}
    };
    result.into()
}

/// Custom attribute for a UEFI executable entrypoint
#[proc_macro_attribute]
pub fn entry(args: TokenStream, input: TokenStream) -> TokenStream {
    // This code is inspired by the approach in this embedded Rust crate:
    // https://github.com/rust-embedded/cortex-m-rt/blob/965bf1e3291571e7e3b34834864117dc020fb391/macros/src/lib.rs#L85

    let mut errors = TokenStream2::new();

    if !args.is_empty() {
        errors.append_all(err!(
            TokenStream2::from(args),
            "Entry attribute accepts no arguments"
        ));
    }

    let mut f = parse_macro_input!(input as ItemFn);

    if let Some(ref abi) = f.sig.abi {
        errors.append_all(err!(abi, "Entry method must have no ABI modifier"));
    }
    if let Some(asyncness) = f.sig.asyncness {
        errors.append_all(err!(asyncness, "Entry method should not be async"));
    }
    if let Some(constness) = f.sig.constness {
        errors.append_all(err!(constness, "Entry method should not be const"));
    }
    if !f.sig.generics.params.is_empty() {
        errors.append_all(err!(
            f.sig.generics.params,
            "Entry method should not be generic"
        ));
    }

    // show most errors at once instead of one by one
    if !errors.is_empty() {
        return errors.into();
    }

    // allow the entry function to be unsafe (by moving the keyword around so that it actually works)
    let unsafety = f.sig.unsafety.take();
    // strip any visibility modifiers
    f.vis = Visibility::Inherited;

    let ident = &f.sig.ident;

    let result = quote! {
        #[export_name = "efi_main"]
        #unsafety extern "efiapi" #f

        // typecheck the function pointer
        const _: #unsafety extern "efiapi" fn(::uefi::Handle, ::uefi::table::SystemTable<::uefi::table::Boot>) -> ::uefi::Status = #ident;
    };
    result.into()
}
