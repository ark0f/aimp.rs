use proc_macro::TokenStream;
use proc_macro2::{Ident, Span};
use quote::quote;
use std::cell::RefCell;
use syn::{parse_macro_input, ItemFn};

thread_local! {
    static TEST_FNS: RefCell<Vec<String>> = RefCell::new(Vec::new());
}

#[proc_macro_attribute]
pub fn test(_args: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    TEST_FNS.with(|fns| fns.borrow_mut().push(input.sig.ident.to_string()));
    (quote! {
        #[allow(dead_code)]
        #input
    })
    .into()
}

#[proc_macro_attribute]
pub fn test_fns(_args: TokenStream, _item: TokenStream) -> TokenStream {
    TEST_FNS
        .with(|fn_names| {
            let fn_names = &*fn_names.borrow();
            let fns: Vec<Ident> = fn_names
                .iter()
                .map(|s| Ident::new(s, Span::call_site()))
                .collect();
            quote! {
                pub fn test_fns() -> std::vec::Vec<aimp::macro_export::tester::TestDescAndFn> {
                    let mut fns = std::vec::Vec::new();
                    #(
                        fns.push(aimp::macro_export::tester::TestDescAndFn {
                            desc: aimp::macro_export::tester::TestDesc {
                                name: aimp::macro_export::tester::StaticTestName(#fn_names),
                                ignore: false,
                                should_panic: aimp::macro_export::tester::ShouldPanic::No,
                                allow_fail: false,
                                test_type: aimp::macro_export::tester::TestType::UnitTest,
                            },
                            testfn: aimp::macro_export::tester::StaticTestFn(#fns),
                        });
                    )*
                    fns
                }
            }
        })
        .into()
}
