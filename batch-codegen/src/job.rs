use inflections::Inflect;
use proc_macro2::{Span, TokenStream};
use quote::ToTokens;
use syn;
use syn::visit_mut::VisitMut;
use syn::spanned::Spanned;
use syn::synom::Synom;

use error::Error;

#[derive(Clone)]
struct JobAttrs {
    attrs: Vec<JobAttr>,
}

#[derive(Clone)]
enum JobAttr {
    Name(syn::LitStr),
    Wrapper(syn::Ident),
    Inject(Vec<syn::Ident>),
}

#[derive(Clone)]
struct Job {
    errors: Vec<Error>,
    visibility: syn::Visibility,
    name: String,
    wrapper: Option<syn::Ident>,
    injected: Vec<syn::Ident>,
    injected_args: Vec<syn::FnArg>,
    serialized_args: Vec<syn::FnArg>,
    original_args: Vec<syn::FnArg>,
    inner_block: Option<syn::Block>,
    ret: Option<syn::Type>,
}

impl JobAttrs {
    fn name(&self) -> Option<String> {
        self.attrs
            .iter()
            .filter_map(|a| match a {
                JobAttr::Name(s) => Some(s.value()),
                _ => None
            })
            .next()
    }

    fn wrapper(&self) -> Option<syn::Ident> {
        self.attrs
            .iter()
            .filter_map(|a| match a {
                JobAttr::Wrapper(i) => Some(i.clone()),
                _ => None,
            })
            .next()
    }

    fn inject(&self) -> Vec<syn::Ident> {
        self.attrs
            .iter()
            .filter_map(|a| match a {
                JobAttr::Inject(i) => Some(i.clone()),
                _ => None,
            })
            .next()
            .unwrap_or_else(Vec::new)
    }
}

impl Synom for JobAttrs {
    named!(parse -> Self, do_parse!(
        attrs: call!(syn::punctuated::Punctuated::<_, Token![,]>::parse_terminated) >>
        (JobAttrs {
            attrs: attrs.into_iter().collect()
        })
    ));
}

impl Synom for JobAttr {
    named!(parse -> Self, alt!(
        do_parse!(
            custom_keyword!(name) >>
            punct!(=) >>
            name: syn!(syn::LitStr) >>
            (name)
        ) => { JobAttr::Name }
        |
        do_parse!(
            custom_keyword!(wrapper) >>
            punct!(=) >>
            wrapper: syn!(syn::Ident) >>
            (wrapper)
        ) => { JobAttr::Wrapper }
        |
        do_parse!(
            custom_keyword!(inject) >>
            punct!(=) >>
            inject: brackets!(call!(syn::punctuated::Punctuated::<_, Token![,]>::parse_terminated)) >>
            (inject.1.into_iter().collect())
        ) => { JobAttr::Inject }
    ));
}

impl Job {
    fn new(attrs: JobAttrs) -> Result<Self, Error> {
        const ERR_MISSING_NAME: &str = "missing mandatory name attribute";

        let errors = Vec::new();
        let visibility = syn::Visibility::Inherited;
        let name = match attrs.name() {
            Some(name) => name,
            None => return Err(Error::new(ERR_MISSING_NAME)),
        };
        let wrapper = attrs.wrapper();
        let injected = attrs.inject();
        let injected_args = Vec::new();
        let serialized_args = Vec::new();
        let original_args = Vec::new();
        let inner_block = None;
        let ret = None;
        Ok(Job {
            errors,
            visibility,
            name,
            wrapper,
            injected,
            injected_args,
            serialized_args,
            original_args,
            inner_block,
            ret,
        })
    }
}

impl VisitMut for Job {
    fn visit_item_fn_mut(&mut self, node: &mut syn::ItemFn) {
        const ERR_ABI: &str = "functions with non-Rust ABI are not supported";

        self.visibility = node.vis.clone();
        if let Some(ref mut it) = node.abi {
            self.errors.push(Error::spanned(ERR_ABI, it.span()));
        };
        if self.wrapper.is_none() {
            let ident = syn::Ident::new(&node.ident.to_string().to_pascal_case(), Span::call_site());
            self.wrapper = Some(ident);
        }
        self.visit_fn_decl_mut(&mut *node.decl);
        self.inner_block = Some((*node.block).clone());
        let wrapper = self.wrapper.as_ref().unwrap();
        let serialized_fields = self.serialized_args
            .iter()
            .fold(TokenStream::new(), |acc, arg| match arg {
                syn::FnArg::Captured(cap) => match cap.pat {
                    syn::Pat::Ident(ref pat) => {
                        let ident = &pat.ident;
                        quote! {
                            #acc
                            #ident,
                        }
                    },
                    _ => acc,
                },
                _ => acc
            });
        node.block = Box::new(parse_quote!({
            #wrapper {
                #serialized_fields
            }
        }));
    }

    fn visit_fn_decl_mut(&mut self, node: &mut syn::FnDecl) {
        const ERR_GENERICS: &str = "functions with generic arguments are not supported";
        const ERR_VARIADIC: &str = "functions with variadic arguments are not supported";
        const ERR_RETURN_TYPE: &str = "functions with non-void retrun types are not supported";

        if node.generics.params.len() > 0 {
            self.errors.push(Error::spanned(ERR_GENERICS, node.generics.span()));
        }
        let (serialized, injected) = node.inputs.clone()
            .into_iter()
            .partition::<Vec<syn::FnArg>, _>(|arg| match arg {
                syn::FnArg::Captured(captured) => {
                    if let syn::Pat::Ident(ref pat) = captured.pat {
                        !self.injected.contains(&pat.ident)
                    } else {
                        true
                    }
                },
                _ => true
            });
        self.serialized_args = serialized.clone();
        self.injected_args = injected;
        self.original_args = node.inputs.clone().into_iter().collect();
        node.inputs = serialized.into_iter().collect();
        if let Some(ref mut it) = node.variadic {
            self.errors.push(Error::spanned(ERR_VARIADIC, it.span()));
        }
        if let syn::ReturnType::Type(_arr, ref ty) = node.output {
            self.ret = Some((**ty).clone());
            self.errors.push(Error::spanned(ERR_RETURN_TYPE, ty.span()));
        }
        // Unwrapping is safe here because we did set it while visiting `ItemFn`.
        let wrapper = self.wrapper.as_ref().unwrap();
        node.output = parse_quote!(-> #wrapper);
    }
}

fn args2fields<'a>(args: impl IntoIterator<Item = &'a syn::FnArg>) -> TokenStream {
    args.into_iter()
        .fold(TokenStream::new(), |acc, arg| match arg {
            syn::FnArg::Captured(cap) => {
                let ident = match cap.pat {
                    syn::Pat::Ident(ref pat) => &pat.ident,
                    _ => return acc,
                };
                let ty = &cap.ty;
                quote! {
                    #acc
                    #ident: #ty,
                }
            },
            _ => acc
        })
}

impl ToTokens for Job {
    fn to_tokens(&self, dst: &mut TokenStream) {
        let vis = &self.visibility;
        let wrapper = self.wrapper.as_ref().unwrap();
        let job_name = &self.name;
        let serialized_fields = args2fields(&self.serialized_args);
        let deserialized_bindings = self.serialized_args
            .iter()
            .fold(TokenStream::new(), |acc, arg| match arg {
                syn::FnArg::Captured(cap) => match cap.pat {
                    syn::Pat::Ident(ref pat) => {
                        let ident = &pat.ident;
                        quote! {
                            #acc
                            let #ident = self.#ident;
                        }
                    },
                    _ => acc,
                },
                _ => acc
            });
        let injected_fields = args2fields(&self.injected_args);
        let injected_bindings = self.injected_args
            .iter()
            .fold(TokenStream::new(), |acc, arg| match arg {
                syn::FnArg::Captured(cap) => {
                    let ident = match cap.pat {
                        syn::Pat::Ident(ref pat) => &pat.ident,
                        _ => return acc,
                    };
                    let ty = &cap.ty;
                    quote! {
                        #acc
                        let #ident = *_ctx.get_local::<#ty>();
                    }
                },
                _ => acc
            });
        let injected_args = self.injected_args
            .iter()
            .fold(TokenStream::new(), |acc, arg| match arg {
                syn::FnArg::Captured(cap) => match cap.pat {
                    syn::Pat::Ident(ref pat) => {
                        let ident = &pat.ident;
                        quote! {
                            #acc
                            #ident,
                        }
                    },
                    _ => acc,
                },
                _ => acc
            });
        let inner_block = if self.ret.is_none() {
            let block = &self.inner_block;
            quote! {
                #block
                ::futures::future::ok(())
            }
        } else {
            let block = &self.inner_block;
            quote!(#block)
        };
        let inner_invoke = quote!(self.perform_now(#injected_args));

        let output = quote! {
            #[derive(Deserialize, Serialize)]
            #vis struct #wrapper {
                #serialized_fields
            }

            impl #wrapper {
                #vis fn perform_now(self, #injected_fields) -> impl ::futures::Future<Item = (), Error = ::failure::Error> {
                    #deserialized_bindings
                    #inner_block
                }
            }

            impl ::batch::Job for #wrapper {
                const NAME: &'static str = #job_name;

                type PerformFuture = Box<::futures::Future<Item = (), Error = ::failure::Error> + Send>;

                /// Performs the job.
                ///
                /// # Panics
                ///
                /// The function will panic if any parameter marked as `injected` cannot be found
                /// in the given Container.
                fn perform(self, _ctx: ::batch::Container) -> Self::PerformFuture {
                    #injected_bindings
                    Box::new(#inner_invoke)
                }
            }
        };
        dst.extend(output);
    }
}

pub fn impl_macro(args: TokenStream, input: TokenStream) -> TokenStream {
    let attrs: JobAttrs = syn::parse2(args).unwrap();
    let mut item: syn::ItemFn = syn::parse2(input).unwrap();
    let mut job = match Job::new(attrs) {
        Ok(job) => job,
        Err(e) => return quote!(#e),
    };
    job.visit_item_fn_mut(&mut item);
    if job.errors.len() > 0 {
        job.errors
            .iter()
            .fold(TokenStream::new(), |mut acc, err| {
                err.to_tokens(&mut acc);
                acc
            })
    } else {
        quote! {
            #job
            #item
        }
    }
}
