mod implementation {
    include!("function_c9c_impl.rs");
    include!("function_c9d_extension.rs");
    include!("function_c14_pointer.rs");
    include!("function_c11c_extension.rs");
    include!("function_c11d_extension.rs");
    include!("function_c14_closure.rs");
    include!("function_c14_bitstruct.rs");
    include!("function_c14_patterns.rs");
    include!("function_c14_scalar.rs");
}

pub(crate) use implementation::lower_function_c14_scalar as lower_function_with_globals;
pub(crate) use implementation::lower_function_c9d as lower_function;
pub(crate) use implementation::lower_module_initializer_c11d as lower_module_initializer;
