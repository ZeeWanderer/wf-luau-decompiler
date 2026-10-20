use std::collections::BTreeMap;

use crate::deserializer::{self, Dialect, bytecode::Bytecode, constant::Constant};
use crate::instruction::Instruction;
use crate::op_code::OpCode;
use crate::{DecompileOptions, panic_payload_message, try_decompile_with_parser};

pub fn try_decompile_warframe_bytecode_with_options(
    bytecode: &[u8],
    script_name: Option<&str>,
    options: DecompileOptions,
) -> Result<String, String> {
    try_decompile_warframe_bytecode_with_symbols(bytecode, script_name, options, &BTreeMap::new())
}

pub fn try_decompile_warframe_bytecode_with_symbols(
    bytecode: &[u8],
    script_name: Option<&str>,
    options: DecompileOptions,
    symbols: &BTreeMap<u32, String>,
) -> Result<String, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let parsed = deserialize(bytecode)?;
        let Bytecode::Chunk(chunk) = &parsed else {
            return Err("Warframe bytecode contains a compiler error".into());
        };
        let mut names = BTreeMap::new();
        for constant in chunk
            .functions
            .iter()
            .flat_map(|function| &function.constants)
        {
            if let Constant::Atom(hash) = constant {
                names.entry(*hash).or_insert_with(|| {
                    symbols
                        .get(hash)
                        .cloned()
                        .unwrap_or_else(|| format!("__wf_atom_{hash:08x}"))
                });
            }
        }

        // Keep presentation names alive across upstream retries. Raw atom identities
        // remain in the parsed dialect; only the lifting input gets string names.
        let string_count = chunk.string_table.len();
        let indices: BTreeMap<_, _> = names
            .keys()
            .enumerate()
            .map(|(index, hash)| (*hash, string_count + index + 1))
            .collect();
        drop(parsed);
        try_decompile_with_parser(bytecode, script_name, options, false, &|| {
            let mut parsed = deserialize(bytecode)?;
            if let Bytecode::Chunk(chunk) = &mut parsed {
                adapt_host_calls(chunk)?;
                chunk
                    .string_table
                    .extend(names.values().map(|name| name.as_bytes()));
                for constant in chunk
                    .functions
                    .iter_mut()
                    .flat_map(|function| &mut function.constants)
                {
                    if let Constant::Atom(hash) = constant {
                        *constant = Constant::String(indices[hash]);
                    }
                }
            }
            Ok(parsed)
        })
        .map(|artifact| artifact.source)
        .map_err(|error| error.to_string())
    }))
    .unwrap_or_else(|payload| {
        Err(format!(
            "panicked: {}",
            panic_payload_message(payload.as_ref())
        ))
    })
}

fn deserialize(bytecode: &[u8]) -> Result<Bytecode<'_>, String> {
    deserializer::deserialize_with_dialect(bytecode, 1, Dialect::Warframe)
}

fn adapt_host_calls(chunk: &mut deserializer::chunk::Chunk<'_>) -> Result<(), String> {
    use OpCode::*;
    for (prototype, function) in chunk.functions.iter_mut().enumerate() {
        for pc in 0..function.instructions.len() {
            match function.instructions[pc] {
                Instruction::BC {
                    op_code:
                        LOP_FASTCALL | LOP_FASTCALL1 | LOP_FASTCALL2 | LOP_FASTCALL2K | LOP_FASTCALL3,
                    c,
                    ..
                } => {
                    if !matches!(
                        function.instructions.get(pc + 1 + usize::from(c)),
                        Some(Instruction::BC {
                            op_code: LOP_CALL | LOP_CALLFB,
                            ..
                        })
                    ) {
                        return Err(format!(
                            "Warframe optimized call has no fallback CALL at prototype {prototype}, pc {pc}"
                        ));
                    }
                    // Warframe's hashed host names are not Luau's builtin table.
                    // Keep the marker's evaluation-order evidence and fallback;
                    // ID 0 makes no claim about the host function's identity.
                    if let Instruction::BC { a, .. } = &mut function.instructions[pc] {
                        *a = 0;
                    }
                }
                Instruction::AD {
                    op_code: prep @ (LOP_FORGPREP_NEXT | LOP_FORGPREP_INEXT),
                    a,
                    d,
                    aux,
                } => {
                    let target = (pc + 1).checked_add_signed(isize::from(d)).ok_or_else(|| {
                        format!(
                            "invalid Warframe iterator target at prototype {prototype}, pc {pc}"
                        )
                    })?;
                    let Some(Instruction::AD {
                        op_code: LOP_FORGLOOP,
                        a: base,
                        aux: loop_aux,
                        ..
                    }) = function.instructions.get_mut(target)
                    else {
                        return Err(format!(
                            "Warframe iterator has no matching FORGLOOP at prototype {prototype}, pc {pc}"
                        ));
                    };
                    let inext = prep == LOP_FORGPREP_INEXT;
                    let count = *loop_aux & 0xff;
                    if *base != a
                        || !(1..=2).contains(&count)
                        || *loop_aux != (if inext { 0x8000_0000 } else { 0 }) | count
                    {
                        return Err(format!(
                            "invalid Warframe specialized iterator at prototype {prototype}, pc {pc}"
                        ));
                    }
                    // Both instructions must select the ordinary iterator path.
                    // Retaining the fast FORGLOOP flag with a generic prep is invalid.
                    *loop_aux = count;
                    function.instructions[pc] = Instruction::AD {
                        op_code: LOP_FORGPREP,
                        a,
                        d,
                        aux,
                    };
                }
                _ => {}
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(opcode: u8, a: u8, b: u8, c: u8) -> [u8; 4] {
        [opcode, a, b, c]
    }

    fn global_fixture(version: u8, hash: u32) -> Vec<u8> {
        let mut bytecode = vec![version, 3, 0, 0, 1, 1, 0, 0, 0, 0, 0, 3];
        bytecode.extend(word(7, 0, 0, 0)); // GETGLOBAL R0, K0
        bytecode.extend(0u32.to_le_bytes());
        bytecode.extend(word(22, 0, 2, 0)); // RETURN R0
        bytecode.extend([1, 1]);
        bytecode.extend(hash.to_le_bytes());
        bytecode.extend([0, 0, 0, 0, 0, 0]);
        bytecode
    }

    #[test]
    fn atoms_keep_identity_until_presentation() {
        for version in [6, 9] {
            let bytes = global_fixture(version, 0x2fdd25fc);
            let Bytecode::Chunk(chunk) = deserialize(&bytes).unwrap() else {
                panic!()
            };
            assert!(matches!(
                chunk.functions[0].constants[0],
                Constant::Atom(0x2fdd25fc)
            ));
            let source = try_decompile_warframe_bytecode_with_options(
                &bytes,
                None,
                DecompileOptions::default(),
            )
            .unwrap();
            assert!(source.contains("return __wf_atom_2fdd25fc"), "{source}");
            let named = try_decompile_warframe_bytecode_with_symbols(
                &bytes,
                None,
                DecompileOptions::default(),
                &BTreeMap::from([(0x2fdd25fc, "module".into())]),
            )
            .unwrap();
            assert!(named.contains("return module"), "{named}");
            assert!(!named.contains("__wf_atom_"));
            let unrelated = try_decompile_warframe_bytecode_with_symbols(
                &bytes,
                None,
                DecompileOptions::default(),
                &BTreeMap::from([(0x12345678, "print".into())]),
            )
            .unwrap();
            assert_eq!(unrelated, source);
        }
    }

    #[test]
    fn booleans_use_dialect_width_and_atoms_are_not_booleans() {
        for (value, expected) in [(0u32, false), (1, true)] {
            let mut encoded = vec![1];
            encoded.extend(value.to_le_bytes());
            let (rest, value) =
                Constant::parse_with_dialect(&encoded, 9, Dialect::Warframe).unwrap();
            assert!(rest.is_empty());
            assert!(matches!(value, Constant::Boolean(value) if value == expected));
        }
        let (rest, value) = Constant::parse_with_dialect(&[1, 1, 99], 9, Dialect::Luau).unwrap();
        assert_eq!(rest, [99]);
        assert!(matches!(value, Constant::Boolean(true)));
        for size in 0..4 {
            assert!(
                Constant::parse_with_dialect(&[1, 0, 0, 0][..size + 1], 9, Dialect::Warframe)
                    .is_err()
            );
        }
    }

    #[test]
    fn truncated_warframe_inputs_fail_without_panicking() {
        let bytes = global_fixture(9, 0x2fdd25fc);
        for end in 0..bytes.len() {
            assert!(
                try_decompile_warframe_bytecode_with_options(
                    &bytes[..end],
                    None,
                    DecompileOptions::default()
                )
                .is_err(),
                "prefix {end}"
            );
        }
    }

    #[test]
    fn host_call_adaptation_keeps_pc_layout_and_requires_the_fallback() {
        use OpCode::*;
        let bytes = global_fixture(9, 0x2fdd25fc);
        let Bytecode::Chunk(mut chunk) = deserialize(&bytes).unwrap() else {
            panic!()
        };
        chunk.functions[0].instructions = vec![
            Instruction::BC {
                op_code: LOP_FASTCALL2,
                a: 40,
                b: 1,
                c: 1,
                aux: 2,
            },
            Instruction::BC {
                op_code: LOP_NOP,
                a: 0,
                b: 0,
                c: 0,
                aux: 0,
            },
            Instruction::BC {
                op_code: LOP_CALL,
                a: 0,
                b: 3,
                c: 2,
                aux: 0,
            },
        ];
        adapt_host_calls(&mut chunk).unwrap();
        let instructions = &chunk.functions[0].instructions;
        assert_eq!(instructions.len(), 3);
        assert!(matches!(
            instructions[0],
            Instruction::BC {
                op_code: LOP_FASTCALL2,
                a: 0,
                b: 1,
                c: 1,
                aux: 2
            }
        ));
        chunk.functions[0].instructions.truncate(2);
        assert!(
            adapt_host_calls(&mut chunk)
                .unwrap_err()
                .contains("no fallback CALL")
        );
    }

    #[test]
    fn iterator_adaptation_preserves_pairing_and_rejects_invalid_metadata() {
        use OpCode::*;
        for (prep, loop_aux) in [
            (LOP_FORGPREP_NEXT, 1),
            (LOP_FORGPREP_NEXT, 2),
            (LOP_FORGPREP_INEXT, 0x8000_0001),
            (LOP_FORGPREP_INEXT, 0x8000_0002),
        ] {
            let bytes = global_fixture(9, 0x2fdd25fc);
            let Bytecode::Chunk(mut chunk) = deserialize(&bytes).unwrap() else {
                panic!()
            };
            chunk.functions[0].instructions = vec![
                Instruction::AD {
                    op_code: prep,
                    a: 1,
                    d: 1,
                    aux: 0,
                },
                Instruction::BC {
                    op_code: LOP_NOP,
                    a: 0,
                    b: 0,
                    c: 0,
                    aux: 0,
                },
                Instruction::AD {
                    op_code: LOP_FORGLOOP,
                    a: 1,
                    d: -2,
                    aux: loop_aux,
                },
            ];
            adapt_host_calls(&mut chunk).unwrap();
            assert_eq!(chunk.functions[0].instructions.len(), 3);
            assert!(matches!(
                chunk.functions[0].instructions[0],
                Instruction::AD {
                    op_code: LOP_FORGPREP,
                    a: 1,
                    d: 1,
                    ..
                }
            ));
            assert!(matches!(
                chunk.functions[0].instructions[2],
                Instruction::AD {
                    op_code: LOP_FORGLOOP,
                    a: 1,
                    d: -2,
                    aux
                } if aux == loop_aux & 0xff
            ));
            chunk.functions[0].instructions[0] = Instruction::AD {
                op_code: prep,
                a: 0,
                d: 1,
                aux: 0,
            };
            assert!(adapt_host_calls(&mut chunk).is_err());
        }
    }

    #[test]
    fn fastcall_keeps_the_hashed_callee_instead_of_inventing_a_builtin_name() {
        use OpCode::*;
        let mut bytes = vec![9, 3, 0, 0, 1, 3, 1, 0, 0, 0, 0, 6];
        bytes.extend(word(LOP_FASTCALL1 as u8, 40, 0, 3));
        bytes.extend(word(LOP_MOVE as u8, 2, 0, 0));
        bytes.extend(word(LOP_GETIMPORT as u8, 1, 1, 0));
        bytes.extend((1u32 << 30).to_le_bytes());
        bytes.extend(word(LOP_CALL as u8, 1, 2, 2));
        bytes.extend(word(LOP_RETURN as u8, 1, 2, 0));
        bytes.extend([2, 1]);
        bytes.extend(0x12345678u32.to_le_bytes());
        bytes.push(4);
        bytes.extend((1u32 << 30).to_le_bytes());
        bytes.extend([0, 0, 0, 0, 0, 0]);
        let source =
            try_decompile_warframe_bytecode_with_options(&bytes, None, DecompileOptions::default())
                .unwrap();
        assert!(source.contains("__wf_atom_12345678("), "{source}");
        assert!(!source.contains("type("), "{source}");
    }
}
