use std::collections::HashSet;

use proc_macro2::{Span, TokenStream};
use quote::{quote_spanned, ToTokens};
use syn::spanned::Spanned;

use crate::attr;
use crate::ctxt::Ctxt;

const NAME: &str = "#[owned]";

enum Access {
    SelfAccess {
        and_token: syn::Token![&],
        self_token: syn::Token![self],
        dot_token: syn::Token![.],
    },
    BindingAccess,
}

enum Binding {
    Field(syn::Ident),
    Index(syn::Index),
}

impl Binding {
    /// Construct binding as a varaible name.
    fn as_variable(&self) -> syn::Ident {
        match self {
            Binding::Field(ident) => ident.clone(),
            Binding::Index(index) => syn::Ident::new(&format!("f{}", index.index), index.span()),
        }
    }

    /// Construct `field: value` syntax.
    fn as_field_value(&self) -> syn::FieldValue {
        match self {
            Binding::Field(ident) => syn::FieldValue {
                attrs: Vec::new(),
                member: syn::Member::Named(ident.clone()),
                colon_token: None,
                expr: syn::Expr::Path(syn::ExprPath {
                    attrs: Vec::new(),
                    qself: None,
                    path: ident.clone().into(),
                }),
            },
            Binding::Index(index) => {
                let ident = syn::Ident::new(&format!("f{}", index.index), index.span());

                syn::FieldValue {
                    attrs: Vec::new(),
                    member: syn::Member::Unnamed(index.clone()),
                    colon_token: Some(<syn::Token![:]>::default()),
                    expr: syn::Expr::Path(syn::ExprPath {
                        attrs: Vec::new(),
                        qself: None,
                        path: ident.into(),
                    }),
                }
            }
        }
    }
}

impl ToTokens for Binding {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            Binding::Field(field) => {
                field.to_tokens(tokens);
            }
            Binding::Index(index) => {
                index.to_tokens(tokens);
            }
        }
    }
}

struct BoundAccess<'a> {
    copy: bool,
    access: &'a Access,
    binding: &'a Binding,
}

impl ToTokens for BoundAccess<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match &self.access {
            Access::SelfAccess {
                and_token,
                self_token,
                dot_token,
            } => {
                if !self.copy {
                    and_token.to_tokens(tokens);
                }

                self_token.to_tokens(tokens);
                dot_token.to_tokens(tokens);
                self.binding.to_tokens(tokens);
            }
            Access::BindingAccess => {
                self.binding.as_variable().to_tokens(tokens);
            }
        }
    }
}

enum Call<'a> {
    Path(&'a syn::Path),
    Copy,
}

impl Call<'_> {
    fn as_tokens(&self, span: Span, access: &BoundAccess<'_>) -> TokenStream {
        match self {
            Call::Path(path) => quote_spanned!(span => #path(#access)),
            Call::Copy => quote_spanned!(span => #access),
        }
    }
}

pub(crate) fn implement(
    cx: &Ctxt,
    attrs: Vec<syn::Attribute>,
    mut item: syn::Item,
) -> Result<TokenStream, ()> {
    let mut output = item.clone();

    let (to_owned_fn, borrow_fn) = match (&mut output, &mut item) {
        (syn::Item::Struct(st), syn::Item::Struct(b_st)) => {
            let container = attr::container(cx, &st.ident, &attrs);
            let container = container?;
            st.ident = container.name;

            let mut to_owned_entries = Vec::new();
            let mut borrow_entries = Vec::new();

            let access = Access::SelfAccess {
                and_token: <syn::Token![&]>::default(),
                self_token: <syn::Token![self]>::default(),
                dot_token: <syn::Token![.]>::default(),
            };

            strip_lifetimes(&mut st.generics);
            process_fields(
                cx,
                &access,
                &mut st.fields,
                &mut b_st.fields,
                &mut to_owned_entries,
                &mut borrow_entries,
            )?;

            let owned_ident = &st.ident;

            let to_owned_fn = quote_spanned! {
                b_st.span() =>
                #[inline]
                fn to_owned(&self) -> Self::Owned {
                    #owned_ident {
                        #(#to_owned_entries,)*
                    }
                }
            };

            let borrow_ident = &b_st.ident;

            let borrow_fn = quote_spanned! {
                st.span() =>
                fn borrow(&self) -> Self::Target<'_> {
                    #borrow_ident {
                        #(#borrow_entries,)*
                    }
                }
            };

            (to_owned_fn, borrow_fn)
        }
        (syn::Item::Enum(en), syn::Item::Enum(b_en)) => {
            let container = attr::container(cx, &en.ident, &attrs);
            let container = container?;
            en.ident = container.name;

            strip_lifetimes(&mut en.generics);

            let mut to_owned_variants = Vec::new();
            let mut borrow_variants = Vec::new();

            let owned_ident = en.ident.clone();
            let borrow_ident = b_en.ident.clone();

            for (variant, b_variant) in en.variants.iter_mut().zip(b_en.variants.iter_mut()) {
                let mut to_owned_entries = Vec::new();
                let mut borrow_entries = Vec::new();

                let access = Access::BindingAccess;
                process_fields(
                    cx,
                    &access,
                    &mut variant.fields,
                    &mut b_variant.fields,
                    &mut to_owned_entries,
                    &mut borrow_entries,
                )?;

                let fields = variant
                    .fields
                    .iter()
                    .enumerate()
                    .map(|(n, f)| match &f.ident {
                        Some(ident) => Binding::Field(ident.clone()),
                        None => Binding::Index(syn::Index::from(n)),
                    });

                let variant_ident = &variant.ident;
                let patterns = fields.clone().map(|b| b.as_field_value());

                to_owned_variants.push(quote_spanned! {
                    variant.span() =>
                    #borrow_ident::#variant_ident { #(#patterns,)* } => {
                        #owned_ident::#variant_ident {
                            #(#to_owned_entries,)*
                        }
                    }
                });

                let patterns = fields.clone().map(|b| b.as_field_value());

                borrow_variants.push(quote_spanned! {
                    variant.span() =>
                    #owned_ident::#variant_ident { #(#patterns,)* } => {
                        #borrow_ident::#variant_ident {
                            #(#borrow_entries,)*
                        }
                    }
                });
            }

            let to_owned_fn = quote_spanned! {
                b_en.span() =>
                #[inline]
                fn to_owned(&self) -> Self::Owned {
                    match self {
                        #(#to_owned_variants,)*
                    }
                }
            };

            let borrow_fn = quote_spanned! {
                en.span() =>
                fn borrow(&self) -> Self::Target<'_> {
                    match self {
                        #(#borrow_variants,)*
                    }
                }
            };

            (to_owned_fn, borrow_fn)
        }
        (_, item) => {
            cx.span_error(
                item.span(),
                format_args!("{NAME}: is only supported on structs."),
            );
            return Err(());
        }
    };

    let (owned_ident, owned_generics) = match &output {
        syn::Item::Struct(st) => (&st.ident, &st.generics),
        syn::Item::Enum(en) => (&en.ident, &en.generics),
        _ => return Err(()),
    };

    let (borrow_ident, borrow_generics) = match &item {
        syn::Item::Struct(st) => (&st.ident, &st.generics),
        syn::Item::Enum(en) => (&en.ident, &en.generics),
        _ => {
            return Err(());
        }
    };

    let (_, to_owned_type_generics, _) = owned_generics.split_for_impl();

    let to_owned = {
        let (impl_generics, type_generics, where_generics) = borrow_generics.split_for_impl();
        let to_owned = &cx.owned_to_owned;

        quote_spanned! {
            item.span() =>
            #[automatically_derived]
            impl #impl_generics #to_owned for #borrow_ident #type_generics #where_generics {
                type Owned = #owned_ident #to_owned_type_generics;
                #to_owned_fn
            }
        }
    };

    let borrow = {
        let mut borrow_generics = borrow_generics.clone();

        // NB: Replace all borrowed lifetimes with `'this`, which borrows from
        // `&self` in `fn borrow`.
        let this_lt = syn::Lifetime::new("'this", Span::call_site());

        for g in &mut borrow_generics.params {
            if let syn::GenericParam::Lifetime(l) = g {
                l.lifetime = this_lt.clone();
            }
        }

        let (_, borrow_return_type_generics, _) = borrow_generics.split_for_impl();

        let (impl_generics, type_generics, where_generics) = owned_generics.split_for_impl();
        let owned_borrow = &cx.owned_borrow;

        quote_spanned! {
            item.span() =>
            #[automatically_derived]
            impl #impl_generics #owned_borrow for #owned_ident #type_generics #where_generics {
                type Target<#this_lt> = #borrow_ident #borrow_return_type_generics;
                #borrow_fn
            }
        }
    };

    let mut stream = TokenStream::new();
    item.to_tokens(&mut stream);
    output.to_tokens(&mut stream);
    to_owned.to_tokens(&mut stream);
    borrow.to_tokens(&mut stream);
    Ok(stream)
}

fn process_fields(
    cx: &Ctxt,
    access: &Access,
    fields: &mut syn::Fields,
    b_fields: &mut syn::Fields,
    to_owned_entries: &mut Vec<TokenStream>,
    borrow_entries: &mut Vec<TokenStream>,
) -> Result<(), ()> {
    for (index, (field, b_field)) in fields.iter_mut().zip(b_fields.iter_mut()).enumerate() {
        let attr = attr::field(cx, &mut field.attrs);
        let attr = attr?;

        attr::strip(&mut b_field.attrs);

        if let Some(meta) = attr.borrowed_meta {
            b_field.attrs.push(syn::Attribute {
                pound_token: syn::token::Pound::default(),
                style: syn::AttrStyle::Outer,
                bracket_token: syn::token::Bracket::default(),
                meta,
            });
        }

        match attr.ty {
            attr::FieldType::Original => {
                // Ensure that the field does not make use of any lifetimes.
                let ignore = HashSet::new();

                ensure_no_lifetimes(cx, field.span(), &field.ty, &ignore);
            }
            attr::FieldType::Type(ty) => {
                field.ty = ty;
            }
        }

        let (to_owned, borrow) = if attr.copy {
            (Call::Copy, Call::Copy)
        } else if attr.is_set {
            (Call::Path(&attr.to_owned), Call::Path(&attr.borrow))
        } else {
            let clone = &cx.clone;
            (Call::Path(clone), Call::Path(clone))
        };

        let binding = match &field.ident {
            Some(ident) => Binding::Field(ident.clone()),
            None => Binding::Index(syn::Index::from(index)),
        };

        let bound = BoundAccess {
            copy: attr.copy,
            access: &access,
            binding: &binding,
        };

        let f = to_owned.as_tokens(field.span(), &bound);
        to_owned_entries.push(quote_spanned!(field.span() => #binding: #f));
        let f = borrow.as_tokens(field.span(), &bound);
        borrow_entries.push(quote_spanned!(field.span() => #binding: #f));
    }

    Ok(())
}

fn ensure_no_lifetimes(cx: &Ctxt, span: Span, ty: &syn::Type, ignore: &HashSet<syn::Ident>) {
    match ty {
        syn::Type::Array(ty) => {
            ensure_no_lifetimes(cx, span, &ty.elem, ignore);
        }
        syn::Type::BareFn(ty) => {
            let mut ignore = ignore.clone();

            // ignore for <'a, 'b, 'c> lifetimes
            if let Some(bound) = &ty.lifetimes {
                for param in &bound.lifetimes {
                    if let syn::GenericParam::Lifetime(lt) = param {
                        ignore.insert(lt.lifetime.ident.clone());
                    }
                }
            }

            for input in &ty.inputs {
                ensure_no_lifetimes(cx, span, &input.ty, &ignore);
            }
        }
        syn::Type::Group(ty) => {
            ensure_no_lifetimes(cx, span, &ty.elem, ignore);
        }
        syn::Type::Reference(ty) => {
            let mut error = if let Some(lt) = &ty.lifetime {
                if ignore.contains(&lt.ident) {
                    return;
                }

                syn::Error::new(lt.span(), format_args!("{NAME}: lifetime not supported."))
            } else {
                syn::Error::new(
                    ty.and_token.span(),
                    format_args!("{NAME}: anonymous references not supported."),
                )
            };

            error.combine(syn::Error::new(
                span,
                "Hint: add #[owned(ty = <type>)] to specify which type to override this field with",
            ));
            cx.error(error);
        }
        syn::Type::Slice(ty) => {
            ensure_no_lifetimes(cx, span, &ty.elem, ignore);
        }
        syn::Type::Tuple(ty) => {
            for ty in &ty.elems {
                ensure_no_lifetimes(cx, span, ty, ignore);
            }
        }
        _ => {}
    }
}

/// Strip lifetime parameters from the given generics.
fn strip_lifetimes(generics: &mut syn::Generics) {
    let mut params = generics.params.clone();
    params.clear();

    for p in &generics.params {
        if !matches!(p, syn::GenericParam::Lifetime(..)) {
            params.push(p.clone());
        }
    }

    generics.params = params;
}
