mod implementation {
    include!("function_c9c_impl.rs");
    include!("function_c9d_extension.rs");
    include!("function_c11c_extension.rs");
    include!("function_c11d_extension.rs");
}

pub(crate) use implementation::lower_function_c11c as lower_function_with_globals;
pub(crate) use implementation::lower_function_c9d as lower_function;
pub(crate) use implementation::lower_module_initializer_c11d as lower_module_initializer;
