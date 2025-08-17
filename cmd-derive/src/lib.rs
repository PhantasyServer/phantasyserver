use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Error, Ident, parse_macro_input, spanned::Spanned};

#[proc_macro_derive(ChatCommand, attributes(help, only_gm, only_not_gm, default, alias))]
pub fn packet_read_write_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    build_parser(&input).unwrap_or_else(|err| err.to_compile_error().into())
}

#[derive(Default)]
enum CmdType {
    #[default]
    AllPlayer,
    GmOnly,
    NotGmOnly,
}

#[derive(Default)]
struct Attributes {
    is_help: bool,
    cmd_type: CmdType,
    doc: String,
    default: bool,
    aliases: Vec<String>,
}

impl Attributes {
    fn parse(attrs: &[syn::Attribute]) -> syn::Result<Self> {
        let mut ret = Self::default();
        for attr in attrs {
            match &attr.meta {
                syn::Meta::Path(path) => {
                    let attribute = path.require_ident()?;
                    let attribute_name = attribute.to_string();
                    match attribute_name.as_str() {
                        "help" => ret.is_help = true,
                        "only_gm" => ret.cmd_type = CmdType::GmOnly,
                        "only_not_gm" => ret.cmd_type = CmdType::NotGmOnly,
                        "default" => ret.default = true,
                        _ => return Err(Error::new(attribute.span(), "Unknown attribute")),
                    }
                }
                syn::Meta::List(list) => {
                    if list.path.require_ident()? == "alias" {
                        let alias: syn::LitStr = attr.parse_args()?;
                        ret.aliases.push(alias.value());
                    }
                }
                syn::Meta::NameValue(meta) => {
                    if let syn::Expr::Lit(expr) = &meta.value {
                        if let syn::Lit::Str(lit_str) = &expr.lit {
                            ret.doc.push_str(&lit_str.value());
                        }
                    }
                }
            }
        }
        Ok(ret)
    }
}

fn ident_to_cmd_name(name: &Ident) -> String {
    let mut out_name = String::new();
    for char in name.to_string().chars() {
        if char.is_uppercase() && !out_name.is_empty() {
            out_name.push('_');
        }
        out_name.push(char);
    }
    out_name.to_lowercase()
}

fn create_help_msg(cmd_name: &str, args: &[Ident], attrs: &Attributes) -> String {
    let mut help = format!("{{yel}} - {cmd_name}");
    for arg in args {
        if attrs.default {
            help.push_str(&format!(" [{arg}]"));
        } else {
            help.push_str(&format!(" {{{arg}}}"));
        }
    }
    if !attrs.aliases.is_empty() {
        help.push_str(" (aliases: ");
        for alias in &attrs.aliases {
            help.push_str(alias);
            help.push_str(", ");
        }
        help.pop();
        help.pop();
        help.push(')');
    }
    help.push_str("{def}");
    if !attrs.doc.is_empty() {
        help.push(':');
        help.push_str(&attrs.doc);
    }
    help.push('\n');
    help
}

fn build_parser(ast: &DeriveInput) -> syn::Result<TokenStream> {
    let name = &ast.ident;
    let Data::Enum(cmds) = &ast.data else {
        return Err(Error::new_spanned(
            ast,
            "Only enums are supported for commands",
        ));
    };

    let mut help_message = String::from("{def}{yel}Available commands:{def}\n");
    let mut gm_only_help = String::new();
    let mut not_gm_only_help = String::new();
    let mut parse_variant_stream = quote! {};

    for variant in &cmds.variants {
        let variant_name = &variant.ident;
        let cmd_name = ident_to_cmd_name(variant_name);
        let attributes = Attributes::parse(&variant.attrs)?;
        let mut variant_stream = quote! {};
        if attributes.is_help {
            parse_variant_stream.extend(quote! {
                (#cmd_name, is_gm) => Ok(Self::#variant_name(Self::get_help(is_gm))),
            });
            continue;
        }
        let mut fields = vec![];
        for field in &variant.fields {
            let name = field
                .ident
                .as_ref()
                .ok_or_else(|| Error::new(variant.span(), "Tuple fields are not supported"))?;
            fields.push(name.clone());
            let ty = &field.ty;
            if attributes.default {
                variant_stream.extend(quote! {
                    let #name = data_stream.next().and_then(|s| s.parse::<#ty>().ok()).unwrap_or_default();
                })
            } else {
                variant_stream.extend(quote! {
                    let #name = data_stream
                        .next()
                        .ok_or(format!("{{red}}Missing argument: {}{{def}}", stringify!(#name)))
                        .and_then(|s| s.parse::<#ty>().map_err(|e| e.to_string()))?;

                })
            }
        }
        if fields.is_empty() {
            variant_stream.extend(quote! {
                Ok(Self::#variant_name)
            })
        } else {
            variant_stream.extend(quote! {
                Ok(Self::#variant_name{#(#fields),*})
            })
        }
        let aliases = &attributes.aliases;
        match attributes.cmd_type {
            CmdType::AllPlayer => {
                parse_variant_stream.extend(quote! {
                    (#cmd_name #(| #aliases)*, _) => {#variant_stream}
                });
                help_message.push_str(&create_help_msg(&cmd_name, &fields, &attributes));
            }
            CmdType::GmOnly => {
                parse_variant_stream.extend(quote! {
                    (#cmd_name #(| #aliases)*, true) => {#variant_stream}
                });
                gm_only_help.push_str(&create_help_msg(&cmd_name, &fields, &attributes));
            }
            CmdType::NotGmOnly => {
                parse_variant_stream.extend(quote! {
                    (#cmd_name #(| #aliases)*, false) => {#variant_stream}
                });
                not_gm_only_help.push_str(&create_help_msg(&cmd_name, &fields, &attributes));
            }
        }
    }

    let code = quote! {
        impl #name {
            fn parse(string: &str, is_gm: bool) -> Result<Self, String> {
                if string.is_empty() {
                    return Err(Self::get_help(is_gm));
                }
                // let mut data_stream = string.split_whitespace();
                let mut data_stream = string
                    .split_terminator('"')
                    .filter(|x| !x.is_empty())
                    .enumerate()
                    .map(|(i, s)| {
                        if i % 2 == 0 {
                            s.split_terminator(' ')
                        } else {
                            s.split_terminator('"')
                        }
                    })
                .flatten();
                let cmd = data_stream.next().unwrap_or_default();
                match (cmd, is_gm) {
                    #parse_variant_stream
                    (unk_cmd, _) => Err(format!("{{red}}Unknown command: {unk_cmd}{{def}}\n{}", Self::get_help(is_gm)))
                }
            }
            fn get_help(is_gm: bool) -> String {
                let mut help = String::from(#help_message);
                if is_gm {
                    help.push_str(#gm_only_help);
                } else {
                    help.push_str(#not_gm_only_help);
                }
                help
            }
        }
    };
    Ok(code.into())
}
