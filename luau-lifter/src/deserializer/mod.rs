use nom::{IResult, bytes::complete::take};
use nom_leb128::leb128_usize;

pub mod bytecode;
pub mod chunk;
pub mod constant;
pub mod function;
mod list;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Dialect {
    Luau,
    Warframe,
}

fn parse_string(input: &[u8]) -> IResult<&[u8], Vec<u8>> {
    let (input, length) = leb128_usize(input)?;
    let (input, bytes) = take(length)(input)?;
    Ok((input, bytes.to_owned()))
}

pub fn deserialize(bytecode: &[u8], encode_key: u8) -> Result<bytecode::Bytecode, String> {
    deserialize_with_dialect(bytecode, encode_key, Dialect::Luau)
}

pub fn deserialize_with_dialect(
    bytecode: &[u8],
    encode_key: u8,
    dialect: Dialect,
) -> Result<bytecode::Bytecode, String> {
    match bytecode::Bytecode::parse(bytecode, encode_key, dialect) {
        Ok((_, deserialized_bytecode)) => Ok(deserialized_bytecode),
        Err(err) => Err(err.to_string()),
    }
}

/*#[test]
fn main() -> anyhow::Result<()> {
    let compiler = Compiler::new()
        .set_debug_level(1).set_optimization_level(2);
    let bytecode = compiler.compile("asd = test");
    println!("{:#?}", bytecode);
    let deserialized = deserialize(&bytecode);
    println!("{:#?}", deserialized);
    Ok(())
}*/
