use proc_macro2::TokenStream;
use quote::ToTokens;
use syn;
use syn::spanned::Spanned;
use syn::synom::{Parser, Synom};

use ::error::Error;

struct ExchangeAttrs {
    ident: syn::Ident,
    attrs: Vec<ExchangeAttr>,
}

enum ExchangeAttr {
    Name(syn::LitStr),
    Kind(ExchangeKind),
    Exclusive(syn::LitBool),
}

enum ExchangeKind {
    Direct,
    Fanout,
    Topic,
    Headers,
    Custom(syn::LitStr),
}

impl ExchangeAttrs {
    fn name(&self) -> Option<&syn::LitStr> {
        self.attrs
            .iter()
            .filter_map(|a| match a {
                ExchangeAttr::Name(s) => Some(s),
                _ => None,
            })
            .next()
    }

    fn kind(&self) -> ExchangeKind {
        ExchangeKind::Direct
    }

    fn exclusive(&self) -> bool {
        false
    }
}

impl Synom for ExchangeAttrs {
    named!(parse -> Self, do_parse!(
        ident: syn!(syn::Ident) >>
        attrs: braces!(call!(syn::punctuated::Punctuated::<_, Token![,]>::parse_terminated)) >>
        (ExchangeAttrs {
            ident,
            attrs: attrs.1.into_iter().collect()
        })
    ));
}

impl Synom for ExchangeAttr {
    named!(parse -> Self, alt!(
        do_parse!(
            custom_keyword!(name) >>
            punct!(=) >>
            name: syn!(syn::LitStr) >>
            (name)
        ) => { ExchangeAttr::Name }
        |
        do_parse!(
            custom_keyword!(kind) >>
            punct!(=) >>
            kind: syn!(ExchangeKind) >>
            (kind)
        ) => { ExchangeAttr::Kind }
        |
        do_parse!(
            custom_keyword!(exclusive) >>
            punct!(=) >>
            exclusive: syn!(syn::LitBool) >>
            (exclusive)
        ) => { ExchangeAttr::Exclusive }
    ));
}

impl Synom for ExchangeKind {
    named!(parse -> Self, alt!(
        custom_keyword!(direct) => { |_| ExchangeKind::Direct }
        |
        custom_keyword!(fanout) => { |_| ExchangeKind::Fanout }
        |
        custom_keyword!(topic) => { |_| ExchangeKind::Topic }
        |
        custom_keyword!(headers) => { |_| ExchangeKind::Headers }
        |
        do_parse!(
            custom_keyword!(custom) >>
            kind: parens!(syn!(syn::LitStr)) >>
            (kind.1)
        ) => { ExchangeKind::Custom }
    ));
}

impl ToTokens for ExchangeKind {
    fn to_tokens(&self, stream: &mut TokenStream) {
        let tokens = match self {
            ExchangeKind::Direct => quote!(::batch::rabbitmq::ExchangeKind::Direct),
            ExchangeKind::Fanout => quote!(::batch::rabbitmq::ExchangeKind::Fanout),
            ExchangeKind::Topic => quote!(::batch::rabbitmq::ExchangeKind::Topic),
            ExchangeKind::Headers => quote!(::batch::rabbitmq::ExchangeKind::Headers),
            ExchangeKind::Custom(kind) => quote!(::batch::rabbitmq::ExchangeKind::Custom(#kind.into())),
        };
        stream.extend(tokens);
    }
}

struct Exchange {
    ident: syn::Ident,
    name: String,
    kind: ExchangeKind,
    exclusive: bool,
}

impl Exchange {
    fn new(attrs: ExchangeAttrs) -> Result<Self, Error> {
        const ERR_MISSING_NAME: &str = "missing mandatory name attribute";

        let exchange = Exchange {
            ident: attrs.ident.clone(),
            name: match attrs.name() {
                Some(name) => name.value(),
                None => return Err(Error::spanned(ERR_MISSING_NAME, attrs.ident.span())),
            },
            kind: attrs.kind(),
            exclusive: attrs.exclusive(),
        };
        Ok(exchange)
    }
}

impl ToTokens for Exchange {
    fn to_tokens(&self, stream: &mut TokenStream) {
        let ident = &self.ident;
        let name = &self.name;
        let kind = &self.kind;
        let exclusive = &self.exclusive;

        let output = quote! {
            pub struct #ident {
                inner: ::batch::rabbitmq::Exchange,
            }

            impl ::batch::Declare for #ident {
                const NAME: &'static str = #name;

                type Input = ::batch::rabbitmq::ExchangeBuilder;

                type Output = ::batch::rabbitmq::Exchange;

                type DeclareFuture = Box<::futures::Future<Item = Self, Error = ::failure::Error> + Send>;

                fn declare(declarator: &mut (impl ::batch::Declarator<Self::Input, Self::Output> + 'static)) -> Self::DeclareFuture {
                    use ::futures::Future;

                    let task = ::batch::rabbitmq::Exchange::builder(Self::NAME.into())
                        .kind(#kind)
                        .exclusive(#exclusive)
                        .declare(declarator)
                        .map(|inner| #ident { inner });
                    Box::new(task)
                }
            }

            impl<J> ::batch::dsl::With<J> for #ident
            where
                J: ::batch::Job
            {
                type Query = ::batch::rabbitmq::Query<J>;

                fn with(&self, job: J) -> Self::Query {
                    self.inner.with(job)
                }
            }
        };
        stream.extend(output)
    }
}

fn parse(input: TokenStream) -> Result<Vec<Exchange>, Error> {
    named!(many_exchanges -> Vec<ExchangeAttrs>, many0!(syn!(ExchangeAttrs)));

    let span = input.span();
    let mut exchanges = Vec::new();
    for attrs in many_exchanges.parse2(input)
        .map_err(|e| Error::spanned(format!("error parsing exchanges configuration: {}", e), span))?
    {
        exchanges.push(Exchange::new(attrs)?);
    }
    Ok(exchanges)
}

pub(crate) fn impl_macro(input: TokenStream) -> TokenStream {
    let exchanges = match parse(input) {
        Ok(exchanges) => exchanges,
        Err(e) => {
            return quote!( #e );
        },
    };
    let mut output = quote!();
    for exchange in exchanges.into_iter().map(|ex| ex.into_token_stream()) {
        output = quote! {
            #output

            #exchange
        };
    }
    output.into()
}
