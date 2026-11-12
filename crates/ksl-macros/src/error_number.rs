//! 错误编号派生.

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, Fields, LitInt, parse_macro_input};

/// 为错误枚举生成 `number`.
pub(super) fn expand(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as syn::DeriveInput);
    let Data::Enum(data) = &input.data else {
        return syn::Error::new_spanned(&input, "ErrorNumber 只能用于枚举")
            .to_compile_error()
            .into();
    };
    let mut next: Option<u16> = None;
    let mut arms = Vec::with_capacity(data.variants.len());
    for variant in &data.variants {
        for attr in &variant.attrs {
            if attr.path().is_ident("phase") {
                let base = match attr.parse_args::<LitInt>() {
                    Ok(base) => base,
                    Err(err) => return err.to_compile_error().into(),
                };
                match base.base10_parse::<u16>() {
                    Ok(value) => next = Some(value),
                    Err(err) => return err.to_compile_error().into(),
                }
            }
        }
        let Some(number) = next else {
            return syn::Error::new_spanned(variant, "首个错误变体必须带有 #[phase(基数)]")
                .to_compile_error()
                .into();
        };
        next = Some(number.saturating_add(1));
        let name = &variant.ident;
        let pattern = match &variant.fields {
            Fields::Named(_) => quote! { Self::#name { .. } },
            Fields::Unnamed(_) => quote! { Self::#name(..) },
            Fields::Unit => quote! { Self::#name },
        };
        arms.push(quote! { #pattern => #number });
    }
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    quote! {
        impl #impl_generics #name #ty_generics #where_clause {
            /// 错误编号: 四位数字, 按阶段与声明顺序生成.
            #[must_use]
            pub fn number(&self) -> u16 {
                match self {
                    #(#arms),*
                }
            }
        }
    }
    .into()
}
