use proc_macro2::TokenStream;
use quote::ToTokens;
use syn;
use syn::synom::{Parser, Synom};
use syn::spanned::Spanned;

use ::error::Error;

#[derive(Clone)]
struct QueueAttrs {
    ident: syn::Ident,
    attrs: Vec<QueueAttr>,
}

#[derive(Clone)]
enum QueueAttr {
    Name(syn::LitStr),
    WithPriorities(syn::LitBool),
    Exclusive(syn::LitBool),
    Bindings(QueueBindings),
}

#[derive(Clone, Default)]
struct QueueBindings {
    bindings: Vec<QueueBinding>
}

#[derive(Clone)]
struct QueueBinding {
    exchange: syn::Path,
    jobs: Vec<syn::Path>,
}

#[derive(Clone)]
struct Queue {
    ident: syn::Ident,
    name: String,
    with_priorities: bool,
    exclusive: bool,
    bindings: QueueBindings,
}

impl QueueAttrs {
    fn name(&self) -> Option<&syn::LitStr> {
        self.attrs
            .iter()
            .filter_map(|a| match a {
                QueueAttr::Name(s) => Some(s),
                _ => None,
            })
            .next()
    }

    fn with_priorities(&self) -> bool {
        self.attrs
            .iter()
            .filter_map(|a| match a {
                QueueAttr::WithPriorities(p) => Some(p.value),
                _ => None,
            })
            .next()
            .unwrap_or(false)
    }

    fn exclusive(&self) -> bool {
        self.attrs
            .iter()
            .filter_map(|a| match a {
                QueueAttr::Exclusive(e) => Some(e.value),
                _ => None,
            })
            .next()
            .unwrap_or(false)
    }

    fn bindings(&self) -> QueueBindings {
        self.attrs
            .iter()
            .filter_map(|a| match a {
                QueueAttr::Bindings(b) => Some(b.clone()),
                _ => None,
            })
            .next()
            .unwrap_or_else(QueueBindings::default)
    }
}

impl Synom for QueueAttrs {
    named!(parse -> Self, do_parse!(
        ident: syn!(syn::Ident) >>
        attrs: braces!(call!(syn::punctuated::Punctuated::<_, Token![,]>::parse_terminated)) >>
        (QueueAttrs {
            ident,
            attrs: attrs.1.into_iter().collect()
        })
    ));
}

impl Synom for QueueAttr {
    named!(parse -> Self, alt!(
        do_parse!(
            custom_keyword!(name) >>
            punct!(=) >>
            name: syn!(syn::LitStr) >>
            (name)
        ) => { QueueAttr::Name }
        |
        do_parse!(
            custom_keyword!(with_priorities) >>
            punct!(=) >>
            kind: syn!(syn::LitBool) >>
            (kind)
        ) => { QueueAttr::WithPriorities }
        |
        do_parse!(
            custom_keyword!(exclusive) >>
            punct!(=) >>
            exclusive: syn!(syn::LitBool) >>
            (exclusive)
        ) => { QueueAttr::Exclusive }
        |
        do_parse!(
            custom_keyword!(bindings) >>
            punct!(=) >>
            bindings: syn!(QueueBindings) >>
            (bindings)
        ) => { QueueAttr::Bindings }
    ));
}

impl Synom for QueueBindings {
    named!(parse -> Self, do_parse!(
        bindings: braces!(call!(syn::punctuated::Punctuated::<_, Token![,]>::parse_terminated)) >>
        (QueueBindings {
            bindings: bindings.1.into_iter().collect()
        })
    ));
}

impl ToTokens for QueueBindings {
    fn to_tokens(&self, dst: &mut TokenStream) {
        let output = self.bindings
            .iter()
            .fold(TokenStream::new(), |mut acc, el| {
                el.to_tokens(&mut acc);
                acc
            });
        dst.extend(output);
    }
}

impl Synom for QueueBinding {
    named!(parse -> Self, do_parse!(
        exchange: syn!(syn::Path) >>
        punct!(=) >>
        jobs: brackets!(syn::punctuated::Punctuated::<_, Token![,]>::parse_terminated) >>
        (QueueBinding {
            exchange,
            jobs: jobs.1.into_iter().collect()
        })
    ));
}

impl ToTokens for QueueBinding {
    fn to_tokens(&self, dst: &mut TokenStream) {
        let exchange = &self.exchange;
        let mut output = quote!();
        for job in &self.jobs {
            output = quote! {
                #output
                .bind::<#exchange, #job>()
            };
        }
        dst.extend(output);
    }
}

impl Queue {
    fn new(attrs: QueueAttrs) -> Result<Self, Error> {
        const ERR_MISSING_NAME: &str = "missing mandatory name attribute";

        let queue = Queue {
            ident: attrs.ident.clone(),
            name: match attrs.name() {
                Some(name) => name.value(),
                None => return Err(Error::spanned(ERR_MISSING_NAME, attrs.ident.span())),
            },
            with_priorities: attrs.with_priorities(),
            exclusive: attrs.exclusive(),
            bindings: attrs.bindings(),
        };
        Ok(queue)
    }
}

impl ToTokens for Queue {
    fn to_tokens(&self, dst: &mut TokenStream) {
        let ident = &self.ident;
        let name = &self.name;
        let bindings = &self.bindings;

        let output = quote! {
            pub struct #ident {
                inner: ::batch::rabbitmq::Queue
            }

            impl ::batch::Declare for #ident {
                const NAME: &'static str = #name;

                type Input = ::batch::rabbitmq::QueueBuilder;

                type Output = ::batch::rabbitmq::Queue;

                type DeclareFuture = Box<::futures::Future<Item = Self, Error = ::failure::Error> + Send>;

                fn declare(declarator: &mut (impl ::batch::Declarator<Self::Input, Self::Output> + 'static)) -> Self::DeclareFuture {
                    use ::futures::Future;

                    let task = ::batch::rabbitmq::Queue::builder(Self::NAME.into())
                        // .with_priorities(true)
                        // .exclusive(true)
                        // .bind::<super::exchanges::Transcoding, super::jobs::ConvertVideoFile>()
                        #bindings
                        .declare(declarator)
                        .map(|inner| #ident { inner });
                    Box::new(task)
                }
            }

            impl ::batch::Callbacks for #ident {
                type Iterator = <::batch::rabbitmq::Queue as ::batch::Callbacks>::Iterator;

                fn callbacks(&self) -> Self::Iterator {
                    self.inner.callbacks()
                }
            }
        };
        dst.extend(output);
    }
}

fn parse(input: TokenStream) -> Result<Vec<Queue>, Error> {
    named!(many_queues -> Vec<QueueAttrs>, many0!(syn!(QueueAttrs)));

    let span = input.span();
    let mut queue = Vec::new();
    for attrs in many_queues.parse2(input)
        .map_err(|e| Error::spanned(format!("error parsing queue configuration: {}", e), span))?
    {
        queue.push(Queue::new(attrs)?);
    }
    Ok(queue)
}

pub(crate) fn impl_macro(input: TokenStream) -> TokenStream {
    let queues = match parse(input) {
        Ok(queues) => queues,
        Err(e) => {
            return quote!( #e );
        },
    };
    let mut output = quote!();
    for queue in queues.into_iter().map(|ex| ex.into_token_stream()) {
        output = quote! {
            #output
            #queue
        };
    }
    output
}
