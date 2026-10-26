use std::io::{Error, ErrorKind, Write};

use crate::{
    bedrock::server::item_stack_request::{
        ItemStackRequest, ItemStackRequestAction, SItemStackRequest,
    },
    codec::var_uint::VarUInt,
    serial::PacketWrite,
};

/// Writes an action's type twice: as a `VarUInt`, then as the byte Pumpkin's
/// reader skips as the legacy action type. The official client sends the same
/// number in both for take, place and swap (the M5 capture); the others are
/// assumed to follow until a capture shows them.
fn action_type<W: Write>(writer: &mut W, action_type: u8) -> Result<(), Error> {
    VarUInt(u32::from(action_type)).write(writer)?;
    action_type.write(writer)
}

/// Mirrors `PacketRead for ItemStackRequestAction`, for the actions the client
/// sends: take, place, swap, drop, destroy, consume, create, and the creative
/// take (`CraftCreative`).
///
/// The others are refused: the client doesn't send them, and for some the
/// reader drops data, such as an auto-craft's ingredients, so no writer could
/// match it.
impl PacketWrite for ItemStackRequestAction {
    fn write<W: Write>(&self, writer: &mut W) -> Result<(), Error> {
        match self {
            Self::Take {
                count,
                source,
                destination,
            } => {
                action_type(writer, 0)?;
                count.write(writer)?;
                source.write(writer)?;
                destination.write(writer)
            }
            Self::Place {
                count,
                source,
                destination,
            } => {
                action_type(writer, 1)?;
                count.write(writer)?;
                source.write(writer)?;
                destination.write(writer)
            }
            Self::Swap { slot1, slot2 } => {
                action_type(writer, 2)?;
                slot1.write(writer)?;
                slot2.write(writer)
            }
            Self::Drop {
                count,
                source,
                randomly,
            } => {
                action_type(writer, 3)?;
                count.write(writer)?;
                source.write(writer)?;
                randomly.write(writer)
            }
            Self::Destroy { count, source } => {
                action_type(writer, 4)?;
                count.write(writer)?;
                source.write(writer)
            }
            Self::Consume { count, source } => {
                action_type(writer, 5)?;
                count.write(writer)?;
                source.write(writer)
            }
            Self::Create { result_index } => {
                action_type(writer, 6)?;
                result_index.write(writer)
            }
            Self::CraftCreative {
                creative_item_id,
                repetitions,
            } => {
                action_type(writer, 12)?;
                creative_item_id.write(writer)?;
                repetitions.write(writer)
            }
            other => Err(Error::new(
                ErrorKind::Unsupported,
                format!("the client can't send the item stack request action {other:?}"),
            )),
        }
    }
}

/// Mirrors `PacketRead for ItemStackRequest`: the request id; a `VarUInt`
/// count and the actions; a `VarUInt` count and the filter strings; then the
/// filter cause as a little-endian `i32`.
impl PacketWrite for ItemStackRequest {
    fn write<W: Write>(&self, writer: &mut W) -> Result<(), Error> {
        self.request_id.write(writer)?;
        VarUInt(self.actions.len() as u32).write(writer)?;
        for action in &self.actions {
            action.write(writer)?;
        }
        VarUInt(self.filter_strings.len() as u32).write(writer)?;
        for filter in &self.filter_strings {
            filter.write(writer)?;
        }
        self.filter_cause.write(writer)
    }
}

/// Mirrors `PacketRead for SItemStackRequest`: a `VarUInt` count, then the
/// requests.
impl PacketWrite for SItemStackRequest {
    fn write<W: Write>(&self, writer: &mut W) -> Result<(), Error> {
        VarUInt(self.requests.len() as u32).write(writer)?;
        for request in &self.requests {
            request.write(writer)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        bedrock::{
            network_item::{ContainerName, FullContainerName},
            server::item_stack_request::ItemStackRequestSlotInfo,
        },
        codec::var_int::VarInt,
        serial::PacketRead,
    };

    /// Reads a capture with Pumpkin's reader, then writes it back with ours.
    fn golden(fixture: &[u8]) -> SItemStackRequest {
        let mut reader = fixture;
        let packet = SItemStackRequest::read(&mut reader).unwrap();
        assert!(reader.is_empty(), "{} bytes left over", reader.len());

        let mut written = Vec::new();
        packet.write(&mut written).unwrap();
        assert_eq!(written, fixture);
        packet
    }

    fn slot(container_name: ContainerName, slot_id: u8, stack_id: i32) -> ItemStackRequestSlotInfo {
        ItemStackRequestSlotInfo {
            container_name: FullContainerName {
                container_name,
                dynamic_id: None,
            },
            slot_id,
            stack_id,
        }
    }

    #[test]
    fn a_take_matches_the_official_client() {
        let packet = golden(include_bytes!("fixtures/item_stack_request-take.bin"));
        let request = &packet.requests[0];
        assert_eq!(request.request_id, VarInt(-3));
        let ItemStackRequestAction::Take {
            count,
            source,
            destination,
        } = &request.actions[0]
        else {
            panic!("expected a take");
        };
        assert_eq!(*count, 1);
        assert_eq!(source.container_name.container_name, ContainerName::HotBar);
        assert_eq!(source.stack_id, 34);
        assert_eq!(
            destination.container_name.container_name,
            ContainerName::Cursor
        );
        assert_eq!(request.filter_cause, -1);
    }

    #[test]
    fn a_place_matches_the_official_client() {
        let packet = golden(include_bytes!("fixtures/item_stack_request-place.bin"));
        assert!(matches!(
            &packet.requests[0].actions[0],
            ItemStackRequestAction::Place { count: 1, destination, .. } if destination.slot_id == 8
        ));
    }

    #[test]
    fn a_swap_matches_the_official_client() {
        let packet = golden(include_bytes!("fixtures/item_stack_request-swap.bin"));
        assert!(matches!(
            &packet.requests[0].actions[0],
            ItemStackRequestAction::Swap { slot1, slot2 }
                if slot1.container_name.container_name == ContainerName::Cursor && slot2.slot_id == 2
        ));
    }

    #[test]
    fn the_other_sent_actions_round_trip() {
        let packet = SItemStackRequest {
            requests: vec![ItemStackRequest {
                request_id: VarInt(-21),
                actions: vec![
                    ItemStackRequestAction::CraftCreative {
                        creative_item_id: VarUInt(117),
                        repetitions: 1,
                    },
                    ItemStackRequestAction::Drop {
                        count: 2,
                        source: slot(ContainerName::HotBar, 3, 40),
                        randomly: true,
                    },
                    ItemStackRequestAction::Destroy {
                        count: 64,
                        source: slot(ContainerName::Cursor, 0, 41),
                    },
                    ItemStackRequestAction::Consume {
                        count: 1,
                        source: slot(ContainerName::Inventory, 9, 42),
                    },
                    ItemStackRequestAction::Create { result_index: 2 },
                ],
                filter_strings: vec!["a filter".to_string()],
                filter_cause: 3,
            }],
        };
        let mut first = Vec::new();
        packet.write(&mut first).unwrap();
        let mut reader = first.as_slice();
        let read = SItemStackRequest::read(&mut reader).unwrap();
        assert!(reader.is_empty(), "{} bytes left over", reader.len());

        let request = &read.requests[0];
        assert_eq!(request.request_id, VarInt(-21));
        assert!(matches!(
            request.actions[0],
            ItemStackRequestAction::CraftCreative {
                creative_item_id: VarUInt(117),
                repetitions: 1
            }
        ));
        assert!(matches!(
            &request.actions[1],
            ItemStackRequestAction::Drop { count: 2, randomly: true, source } if source.stack_id == 40
        ));
        assert!(matches!(
            request.actions[4],
            ItemStackRequestAction::Create { result_index: 2 }
        ));
        assert_eq!(request.filter_strings, ["a filter"]);
        assert_eq!(request.filter_cause, 3);

        let mut second = Vec::new();
        read.write(&mut second).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn an_action_the_client_never_sends_is_refused() {
        let error = ItemStackRequestAction::LabTableCombine
            .write(&mut Vec::new())
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Unsupported);
    }
}
