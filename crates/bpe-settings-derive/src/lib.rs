//! `#[derive(Settings)]` and `#[derive(Choice)]` for `bpe-core`.
//!
//! `Settings` reads `#[setting(...)]` on each field and the field's `///` doc text, and writes the
//! schema, sparse saving and forgiving loading. `Choice` names each unit variant of an enum in
//! snake_case. Generated code refers to `::bpe_core`, so it works inside that crate too.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::{Data, DeriveInput, Expr, Fields, LitStr, parse_macro_input, spanned::Spanned};

#[proc_macro_derive(Settings, attributes(setting))]
pub fn derive_settings(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    settings(&input).unwrap_or_else(|e| e.to_compile_error()).into()
}

#[proc_macro_derive(Choice)]
pub fn derive_choice(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    choice(&input).unwrap_or_else(|e| e.to_compile_error()).into()
}

/// What one field's `#[setting(...)]` says.
#[derive(Default)]
struct FieldAttrs {
    label: Option<LitStr>,
    unit: Option<LitStr>,
    group: Option<LitStr>,
    when: Option<LitStr>,
    range: Option<(Expr, Expr)>,
    dice: Option<(Expr, Expr)>,
    simple: bool,
    keyframe: bool,
    random: bool,
    pulse: bool,
    structural: bool,
    /// The field is a group of settings (such as `Paint`) that expands to several prefixed ones.
    paint: bool,
    prefix: Option<LitStr>,
    follows: Option<LitStr>,
    screen_only: bool,
    /// The group offers no per-instance colour variation.
    no_vary: bool,
}

/// The `(min, max)` after an option such as `range`.
fn pair(meta: &syn::meta::ParseNestedMeta) -> syn::Result<(Expr, Expr)> {
    let content;
    syn::parenthesized!(content in meta.input);
    let min: Expr = content.parse()?;
    content.parse::<syn::Token![,]>()?;
    let max: Expr = content.parse()?;
    Ok((min, max))
}

fn field_attrs(field: &syn::Field) -> syn::Result<FieldAttrs> {
    let mut a = FieldAttrs::default();
    for attr in field.attrs.iter().filter(|attr| attr.path().is_ident("setting")) {
        attr.parse_nested_meta(|meta| {
            let p = &meta.path;
            if p.is_ident("label") {
                a.label = Some(meta.value()?.parse()?);
            } else if p.is_ident("unit") {
                a.unit = Some(meta.value()?.parse()?);
            } else if p.is_ident("group") {
                a.group = Some(meta.value()?.parse()?);
            } else if p.is_ident("when") {
                a.when = Some(meta.value()?.parse()?);
            } else if p.is_ident("range") {
                a.range = Some(pair(&meta)?);
            } else if p.is_ident("simple") {
                a.simple = true;
            } else if p.is_ident("keyframe") {
                a.keyframe = true;
            } else if p.is_ident("random") {
                // `random` on its own rolls over the whole range; `random(min, max)` narrows it.
                a.random = true;
                if meta.input.peek(syn::token::Paren) {
                    a.dice = Some(pair(&meta)?);
                }
            } else if p.is_ident("pulse") {
                a.pulse = true;
            } else if p.is_ident("structural") {
                a.structural = true;
            } else if p.is_ident("paint") {
                a.paint = true;
            } else if p.is_ident("prefix") {
                a.prefix = Some(meta.value()?.parse()?);
            } else if p.is_ident("follows") {
                a.follows = Some(meta.value()?.parse()?);
            } else if p.is_ident("screen_only") {
                a.screen_only = true;
            } else if p.is_ident("no_vary") {
                a.no_vary = true;
            } else {
                return Err(meta.error("unknown setting option"));
            }
            Ok(())
        })?;
    }
    Ok(a)
}

/// The field's `///` doc lines joined into one line.
fn help(field: &syn::Field) -> String {
    let mut lines = Vec::new();
    for attr in &field.attrs {
        if let syn::Meta::NameValue(nv) = &attr.meta
            && nv.path.is_ident("doc")
            && let Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) = &nv.value
        {
            let line = s.value().trim().to_owned();
            if !line.is_empty() {
                lines.push(line);
            }
        }
    }
    lines.join(" ")
}

fn settings(input: &DeriveInput) -> syn::Result<Tokens> {
    let name = &input.ident;
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new(input.span(), "Settings can only be derived for structs"));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new(input.span(), "Settings needs a struct with named fields"));
    };
    let (mut defs, mut reads, mut writes) = (Vec::new(), Vec::new(), Vec::new());
    for field in &fields.named {
        let ident = field.ident.as_ref().expect("named field");
        let ty = &field.ty;
        let key = ident.to_string();
        let a = field_attrs(field)?;
        let label = a.label.ok_or_else(|| syn::Error::new(field.span(), "setting needs a label = \"...\""))?;
        let unit = a.unit.map(|s| s.value()).unwrap_or_default();
        let group = a.group.map(|s| s.value()).unwrap_or_default();
        let when = a.when.map(|s| s.value()).unwrap_or_default();
        if a.paint {
            let prefix = a.prefix.map(|s| s.value()).unwrap_or_default();
            let follows: Vec<String> =
                a.follows.map(|s| s.value().split('|').map(|n| n.trim().to_owned()).collect()).unwrap_or_default();
            let (screen_only, simple, vary) = (a.screen_only, a.simple, !a.no_vary);
            let opts = quote! {
                ::bpe_core::GroupOpts {
                    prefix: #prefix,
                    label: #label,
                    group: #group,
                    when: #when,
                    follows: &[#(#follows),*],
                    screen_only: #screen_only,
                    simple: #simple,
                    vary: #vary,
                }
            };
            defs.push(quote! {
                v.extend(<#ty as ::bpe_core::SettingGroup>::group_defs(&#opts, &d.#ident));
            });
            reads.push(quote! {
                s.#ident = <#ty as ::bpe_core::SettingGroup>::group_read(&#opts, &s.#ident, p);
            });
            writes.push(quote! {
                ::bpe_core::SettingGroup::group_write(&self.#ident, &#opts, &d.#ident, &mut p);
            });
            continue;
        }
        let help = help(field);
        let span = |r: Option<(Expr, Expr)>| match r {
            Some((min, max)) => quote!(::std::option::Option::Some(((#min) as f64, (#max) as f64))),
            None => quote!(::std::option::Option::None),
        };
        let (range, dice) = (span(a.range), span(a.dice));
        let depth = if a.simple { quote!(::bpe_core::Depth::Simple) } else { quote!(::bpe_core::Depth::More) };
        let (keyframe, random, pulse, structural) = (a.keyframe, a.random, a.pulse, a.structural);
        defs.push(quote! {
            v.push(::bpe_core::SettingDef {
                key: #key,
                label: #label,
                help: #help,
                ty: <#ty as ::bpe_core::SettingValue>::value_type(),
                default: ::bpe_core::SettingValue::to_value(&d.#ident),
                range: #range,
                dice: #dice,
                unit: #unit,
                group: #group,
                depth: #depth,
                flags: ::bpe_core::Flags { keyframe: #keyframe, random: #random, pulse: #pulse, structural: #structural },
                when: #when,
            });
        });
        reads.push(quote! {
            if let ::std::option::Option::Some(v) =
                p.get(#key).and_then(<#ty as ::bpe_core::SettingValue>::from_value)
            {
                s.#ident = v;
            }
        });
        writes.push(quote! {
            let v = ::bpe_core::SettingValue::to_value(&self.#ident);
            if v != ::bpe_core::SettingValue::to_value(&d.#ident) {
                p.set(#key, v);
            }
        });
    }
    Ok(quote! {
        impl ::bpe_core::Settings for #name {
            fn defs() -> &'static [::bpe_core::SettingDef] {
                static DEFS: ::std::sync::OnceLock<::std::vec::Vec<::bpe_core::SettingDef>> =
                    ::std::sync::OnceLock::new();
                DEFS.get_or_init(|| {
                    let d = <Self as ::std::default::Default>::default();
                    let mut v: ::std::vec::Vec<::bpe_core::SettingDef> = ::std::vec::Vec::new();
                    #(#defs)*
                    let mut keys = ::std::collections::BTreeSet::new();
                    for def in &v {
                        ::std::debug_assert!(
                            keys.insert(def.key),
                            "two settings share the key \"{}\"",
                            def.key
                        );
                    }
                    v
                })
            }

            fn from_params(p: &::bpe_core::Params) -> Self {
                let mut s = <Self as ::std::default::Default>::default();
                #(#reads)*
                s
            }

            fn to_params(&self) -> ::bpe_core::Params {
                let d = <Self as ::std::default::Default>::default();
                let mut p = ::bpe_core::Params::default();
                #(#writes)*
                p
            }
        }
    })
}

/// `RoundBox` to `round_box`, `HSV` to `hsv`, `GPULiquid` to `gpu_liquid`. A word starts at an
/// uppercase letter that follows a lowercase letter or digit, or that starts a run of lowercase
/// letters after other uppercase ones.
fn snake_case(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_uppercase() && i > 0 {
            let prev = chars[i - 1];
            let next_is_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            if prev.is_lowercase() || prev.is_ascii_digit() || (prev.is_uppercase() && next_is_lower) {
                out.push('_');
            }
        }
        out.extend(c.to_lowercase());
    }
    out
}

fn choice(input: &DeriveInput) -> syn::Result<Tokens> {
    let name = &input.ident;
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new(input.span(), "Choice can only be derived for enums"));
    };
    let mut idents = Vec::new();
    let mut names = Vec::new();
    for v in &data.variants {
        if !matches!(v.fields, Fields::Unit) {
            return Err(syn::Error::new(v.span(), "Choice variants can't hold data"));
        }
        idents.push(&v.ident);
        names.push(snake_case(&v.ident.to_string()));
    }
    Ok(quote! {
        impl ::bpe_core::Choice for #name {
            const NAMES: &'static [&'static str] = &[#(#names),*];

            fn name(self) -> &'static str {
                match self {
                    #(Self::#idents => #names,)*
                }
            }

            fn from_name(n: &str) -> ::std::option::Option<Self> {
                match n {
                    #(#names => ::std::option::Option::Some(Self::#idents),)*
                    _ => ::std::option::Option::None,
                }
            }
        }
    })
}
