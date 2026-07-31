use std::sync::{Arc, Mutex, OnceLock};

use crate::{
    abi::{self, Token},
    bridge::NodeRecord,
    error::FfiError,
    frame::OwnedFrame,
};

pub enum Entry {
    Runtime,
    Session,
    Frame(OwnedFrame),
    Node(Arc<NodeRecord>),
}

impl Entry {
    fn kind(&self) -> Kind {
        match self {
            Self::Runtime => Kind::Runtime,
            Self::Session => Kind::Session,
            Self::Frame(_) => Kind::Frame,
            Self::Node(_) => Kind::Node,
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum Kind {
    Runtime,
    Session,
    Frame,
    Node,
}

struct Slot {
    generation: u64,
    entry: Option<Entry>,
    retired_generation: u64,
}

#[derive(Default)]
struct Registry {
    slots: Vec<Slot>,
}

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}

pub fn insert(entry: Entry) -> Token {
    let mut registry = registry().lock().unwrap_or_else(|error| error.into_inner());
    if let Some((index, slot)) = registry
        .slots
        .iter_mut()
        .enumerate()
        .find(|(_, slot)| slot.entry.is_none())
    {
        slot.generation = slot.generation.wrapping_add(1).max(1);
        slot.entry = Some(entry);
        return Token {
            slot: index as u64,
            generation: slot.generation,
        };
    }
    registry.slots.push(Slot {
        generation: 1,
        entry: Some(entry),
        retired_generation: 0,
    });
    Token {
        slot: (registry.slots.len() - 1) as u64,
        generation: 1,
    }
}

pub fn contains(token: Token, kind: Kind) -> Result<(), FfiError> {
    let registry = registry().lock().unwrap_or_else(|error| error.into_inner());
    let slot = slot(&registry, token)?;
    match slot.entry.as_ref() {
        Some(entry) if entry.kind() == kind => Ok(()),
        Some(_) => Err(FfiError::handle(
            abi::INVALID_HANDLE,
            "handle has the wrong kind",
        )),
        None if slot.retired_generation == token.generation => {
            Err(FfiError::handle(abi::CLOSED, "handle was released"))
        }
        None => Err(FfiError::handle(
            abi::INVALID_HANDLE,
            "handle generation is stale",
        )),
    }
}

pub fn node(token: Token) -> Result<Arc<NodeRecord>, FfiError> {
    let registry = registry().lock().unwrap_or_else(|error| error.into_inner());
    let slot = slot(&registry, token)?;
    match slot.entry.as_ref() {
        Some(Entry::Node(node)) => Ok(node.clone()),
        Some(_) => Err(FfiError::handle(
            abi::INVALID_HANDLE,
            "handle has the wrong kind",
        )),
        None if slot.retired_generation == token.generation => {
            Err(FfiError::handle(abi::CLOSED, "node was released"))
        }
        None => Err(FfiError::handle(
            abi::INVALID_HANDLE,
            "node handle is stale",
        )),
    }
}

pub fn release(token: Token, kind: Kind) -> Result<Entry, FfiError> {
    let mut registry = registry().lock().unwrap_or_else(|error| error.into_inner());
    let slot = slot_mut(&mut registry, token)?;
    match slot.entry.as_ref() {
        Some(entry) if entry.kind() != kind => {
            return Err(FfiError::handle(
                abi::INVALID_HANDLE,
                "handle has the wrong kind",
            ))
        }
        None if slot.retired_generation == token.generation => {
            return Err(FfiError::handle(abi::CLOSED, "handle was already released"))
        }
        None => {
            return Err(FfiError::handle(
                abi::INVALID_HANDLE,
                "handle generation is stale",
            ))
        }
        Some(_) => {}
    }
    slot.retired_generation = token.generation;
    Ok(slot.entry.take().expect("entry checked above"))
}

fn slot(registry: &Registry, token: Token) -> Result<&Slot, FfiError> {
    let index = usize::try_from(token.slot)
        .map_err(|_| FfiError::handle(abi::INVALID_HANDLE, "handle slot overflows usize"))?;
    let slot = registry
        .slots
        .get(index)
        .ok_or_else(|| FfiError::handle(abi::INVALID_HANDLE, "handle slot does not exist"))?;
    if slot.generation != token.generation {
        return Err(FfiError::handle(
            abi::INVALID_HANDLE,
            "handle generation is stale",
        ));
    }
    Ok(slot)
}

fn slot_mut(registry: &mut Registry, token: Token) -> Result<&mut Slot, FfiError> {
    let index = usize::try_from(token.slot)
        .map_err(|_| FfiError::handle(abi::INVALID_HANDLE, "handle slot overflows usize"))?;
    let slot = registry
        .slots
        .get_mut(index)
        .ok_or_else(|| FfiError::handle(abi::INVALID_HANDLE, "handle slot does not exist"))?;
    if slot.generation != token.generation {
        return Err(FfiError::handle(
            abi::INVALID_HANDLE,
            "handle generation is stale",
        ));
    }
    Ok(slot)
}
