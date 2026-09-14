from pathlib import Path
p = Path('tools/fir_step13.py')
s = p.read_text()
old = '''replace_once(\n    fir,\n    ''' + "'''    Convert {\\n        value: FirValueId,\\n        target: Ty,\\n    },\\n    MakeArray {'''" + ''',\n    ''' + "'''    Convert {\\n        value: FirValueId,\\n        target: Ty,\\n    },\\n    BitStructStorage {\\n        value: FirValueId,\\n        storage: Ty,\\n    },\\n    BitStructFromStorage {\\n        value: FirValueId,\\n        bitstruct: DefId,\\n    },\\n    BitFieldCheck {\\n        value: FirValueId,\\n        width: u32,\\n    },\\n    MakeArray {'''" + ''')'''
new = '''replace_once(\n    fir,\n    ''' + "'''    Convert {\\n        value: FirValueId,\\n        target: Ty,\\n    },\\n    PointerOffset {'''" + ''',\n    ''' + "'''    Convert {\\n        value: FirValueId,\\n        target: Ty,\\n    },\\n    BitStructStorage {\\n        value: FirValueId,\\n        storage: Ty,\\n    },\\n    BitStructFromStorage {\\n        value: FirValueId,\\n        bitstruct: DefId,\\n    },\\n    BitFieldCheck {\\n        value: FirValueId,\\n        width: u32,\\n    },\\n    PointerOffset {'''" + ''')'''
if old not in s:
    raise SystemExit('old FIR migration block not found')
p.write_text(s.replace(old, new, 1))
