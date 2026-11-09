use heck::ToUpperCamelCase;
use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use quote::{format_ident, quote};
use syn::{FnArg, ItemFn, LitStr, Pat, ReturnType, parse_macro_input};

/// Generate a tool wrapper for a function returning `Result<T, E>`.
/// The successful value must implement `Serialize` and `JsonSchema`, 
/// and the error must convert into `tool::ToolError`.
#[proc_macro_attribute]
pub fn tool(attributes: TokenStream, item: TokenStream) -> TokenStream {
    let mut name = None;
    let mut description = None;

    let parser = syn::meta::parser(|meta| {
        let target = if meta.path.is_ident("name") {
            &mut name
        } else if meta.path.is_ident("description") {
            &mut description
        } else {
            return Err(meta.error("expected `name` or `description`"));
        };
        if target.is_some() {
            return Err(meta.error("duplicate tool option"));
        }
        *target = Some(meta.value()?.parse::<LitStr>()?);
        Ok(())
    });

    parse_macro_input!(attributes with parser);

    let function = parse_macro_input!(item as ItemFn);

    expand(function, name, description)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand(
    function: ItemFn,
    name: Option<LitStr>,
    description: Option<LitStr>,
) -> syn::Result<proc_macro2::TokenStream> {
    let signature = &function.sig;

    if !signature.generics.params.is_empty()
        || signature.generics.where_clause.is_some()
        || signature.unsafety.is_some()
        || signature.constness.is_some()
        || signature.abi.is_some()
        || signature.variadic.is_some()
    {
        return Err(syn::Error::new_spanned(
            signature,
            "tools require a non-generic, safe Rust function",
        ));
    }

    let runtime = match crate_name("tool") {
        Ok(FoundCrate::Itself) => quote!(::tool),
        Ok(FoundCrate::Name(name)) => {
            let ident = format_ident!("{name}");
            quote!(::#ident)
        }
        Err(error) => return Err(syn::Error::new_spanned(signature, error.to_string())),
    };

    let function_name = &signature.ident;

    let base = function_name
        .to_string()
        .trim_start_matches("r#")
        .to_upper_camel_case();

    let tool = format_ident!("{base}Tool");

    let input = format_ident!("{base}ToolInput");

    let visibility = &function.vis;

    let name = name.unwrap_or_else(|| {
        LitStr::new(
            function_name.to_string().trim_start_matches("r#"),
            function_name.span(),
        )
    });

    let description = description.unwrap_or_else(|| LitStr::new("", function_name.span()));

    let mut fields = Vec::new();

    let mut arguments = Vec::new();

    for argument in &signature.inputs {
        let FnArg::Typed(argument) = argument else {
            return Err(syn::Error::new_spanned(
                argument,
                "tools cannot have a self receiver",
            ));
        };

        let Pat::Ident(pattern) = argument.pat.as_ref() else {
            return Err(syn::Error::new_spanned(
                &argument.pat,
                "tool arguments must be named identifiers",
            ));
        };

        if pattern.by_ref.is_some() || pattern.subpat.is_some() {
            return Err(syn::Error::new_spanned(
                pattern,
                "tool arguments must be named identifiers",
            ));
        }

        if let syn::Type::Reference(_) = argument.ty.as_ref() {
            return Err(syn::Error::new_spanned(
                &argument.ty,
                "tool arguments must be owned types (use String instead of &str)",
            ));
        }

        let ident = &pattern.ident;
        let ty = &argument.ty;
        fields.push(quote!(pub #ident: #ty));
        arguments.push(quote!(input.#ident));
    }

    let output = result_output(&signature.output)?;

    let await_call = signature.asyncness.map(|_| quote!(.await));

    let serde_path = LitStr::new(
        &format!("{}::__private::serde", runtime.to_string().replace(' ', "")),
        function_name.span(),
    );

    let schemars_path = LitStr::new(
        &format!(
            "{}::__private::schemars",
            runtime.to_string().replace(' ', "")
        ),
        function_name.span(),
    );

    Ok(quote! {
        #function

        #[derive(Debug, Clone, Copy, Default)]
        #visibility struct #tool;

        #[derive(#runtime::__private::serde::Deserialize, #runtime::__private::schemars::JsonSchema)]
        #[serde(crate = #serde_path, deny_unknown_fields)]
        #[schemars(crate = #schemars_path)]
        #visibility struct #input { #(#fields,)* }

        #[#runtime::async_trait]
        impl #runtime::Tool for #tool {
            type Input = #input;
            type Output = #output;

            fn name(&self) -> &str { #name }
            fn description(&self) -> &str { #description }

            async fn call(&self, input: Self::Input) -> Result<Self::Output, #runtime::ToolError> {
                #function_name(#(#arguments),*) #await_call
                    .map_err(::core::convert::Into::<#runtime::ToolError>::into)
            }
        }
    })
}

fn result_output(output: &ReturnType) -> syn::Result<&syn::Type> {
    let invalid = || {
        syn::Error::new_spanned(
            output,
            "tools must return Result<T, E> (or a qualified Result<T> alias)",
        )
    };
    let ReturnType::Type(_, ty) = output else {
        return Err(invalid());
    };
    let syn::Type::Path(path) = ty.as_ref() else {
        return Err(invalid());
    };
    let segment = path.path.segments.last().ok_or_else(invalid)?;
    if segment.ident != "Result" {
        return Err(invalid());
    }
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return Err(invalid());
    };
    if !(1..=2).contains(&arguments.args.len()) {
        return Err(invalid());
    }
    match arguments.args.first() {
        Some(syn::GenericArgument::Type(ty)) => Ok(ty),
        _ => Err(invalid()),
    }
}
