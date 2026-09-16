use forge_fir::{
    AbiDecomposer, AbiPieceKind, AbiTarget, LayoutEngine, LayoutTarget, Ty, TypeDefinitionTable,
};

fn closure_ty() -> Ty {
    Ty::Closure {
        params: vec![Ty::Int {
            signed: true,
            width: forge_fir::IntWidth::W32,
        }],
        result: Box::new(Ty::Int {
            signed: true,
            width: forge_fir::IntWidth::W32,
        }),
    }
}

#[test]
fn closure_layout_is_one_target_pointer() {
    let definitions = TypeDefinitionTable::new();
    for (bits, bytes) in [(32, 4), (64, 8)] {
        let mut layouts = LayoutEngine::new(LayoutTarget::new(bits), &definitions);
        let layout = layouts.layout_of(&closure_ty()).expect("closure layout");
        assert_eq!(layout.size, bytes);
        assert_eq!(layout.align, bytes);
    }
}

#[test]
fn closure_abi_is_one_pointer_piece_on_sia32_and_native64() {
    let definitions = TypeDefinitionTable::new();
    for target in [AbiTarget::sia32(), AbiTarget::native64()] {
        let mut abi = AbiDecomposer::new(target, &definitions).expect("ABI decomposer");
        let decomposition = abi.decompose(&closure_ty()).expect("closure ABI");
        assert_eq!(decomposition.pieces.len(), 1);
        assert_eq!(decomposition.pieces[0].kind, AbiPieceKind::Pointer);
        assert_eq!(decomposition.pieces[0].bits, target.pointer_bits);
    }
}
