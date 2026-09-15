mod implementation {
    include!("function_c9c_impl.rs");
    include!("function_c9d_extension.rs");
}

pub(crate) use implementation::lower_function_c9d as lower_function;
