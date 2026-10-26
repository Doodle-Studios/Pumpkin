use std::io::{Error, ErrorKind, Read};

use crate::{
    bedrock::client::item_stack_response::{
        CItemStackResponse, ItemStackResponseContainerInfo, ItemStackResponseInfo,
        ItemStackResponseSlotInfo,
    },
    codec::{var_int::VarInt, var_uint::VarUInt},
    serial::PacketRead,
};

/// The most responses the writer sends in one packet.
const MAX_RESPONSES: u32 = 4096;
/// The most containers one response may claim. A request touches a handful.
const MAX_CONTAINERS: u32 = 256;

/// Mirrors `PacketWrite for ItemStackResponseSlotInfo`: the requested slot, the
/// slot and the amount as bytes; a `bool`, then the stack's net id when it's
/// positive (otherwise it reads as 0); the custom name, the filtered custom
/// name, and the durability correction.
impl PacketRead for ItemStackResponseSlotInfo {
    fn read<R: Read>(reader: &mut R) -> Result<Self, Error> {
        let requested_slot = u8::read(reader)?;
        let slot = u8::read(reader)?;
        let amount = u8::read(reader)?;
        let item_stack_net_id = if bool::read(reader)? {
            VarInt::read(reader)?
        } else {
            VarInt(0)
        };
        Ok(Self {
            requested_slot,
            slot,
            amount,
            item_stack_net_id,
            custom_name: String::read(reader)?,
            filtered_custom_name: String::read(reader)?,
            durability_correction: VarInt::read(reader)?,
        })
    }
}

/// Mirrors `PacketWrite for ItemStackResponseInfo`: the result, the client's
/// request id, then a `bool` saying whether a `VarUInt` count of containers
/// and the containers follow.
impl PacketRead for ItemStackResponseInfo {
    fn read<R: Read>(reader: &mut R) -> Result<Self, Error> {
        let result = u8::read(reader)?;
        let client_request_id = VarInt::read(reader)?;
        let containers = if bool::read(reader)? {
            let count = VarUInt::read(reader)?.0;
            if count > MAX_CONTAINERS {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    format!("container count {count} exceeds limit of {MAX_CONTAINERS}"),
                ));
            }
            (0..count)
                .map(|_| ItemStackResponseContainerInfo::read(reader))
                .collect::<Result<_, _>>()?
        } else {
            Vec::new()
        };
        Ok(Self {
            result,
            client_request_id,
            containers,
        })
    }
}

/// Mirrors `PacketWrite for CItemStackResponse`: a `VarUInt` count, then the
/// responses.
impl PacketRead for CItemStackResponse {
    fn read<R: Read>(reader: &mut R) -> Result<Self, Error> {
        let count = VarUInt::read(reader)?.0;
        if count > MAX_RESPONSES {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("item stack response count {count} exceeds limit of {MAX_RESPONSES}"),
            ));
        }
        let responses = (0..count)
            .map(|_| ItemStackResponseInfo::read(reader))
            .collect::<Result<_, _>>()?;
        Ok(Self { responses })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        bedrock::network_item::{ContainerName, FullContainerName},
        serial::PacketWrite,
    };

    fn slot(slot: u8, amount: u8, item_stack_net_id: i32) -> ItemStackResponseSlotInfo {
        ItemStackResponseSlotInfo {
            requested_slot: slot,
            slot,
            amount,
            item_stack_net_id: VarInt(item_stack_net_id),
            custom_name: "Named".to_string(),
            filtered_custom_name: "N****".to_string(),
            durability_correction: VarInt(-3),
        }
    }

    #[test]
    fn responses_round_trip() {
        let packet = CItemStackResponse {
            responses: vec![
                ItemStackResponseInfo {
                    result: 0,
                    client_request_id: VarInt(-3),
                    containers: vec![
                        ItemStackResponseContainerInfo {
                            full_container_name: FullContainerName {
                                container_name: ContainerName::HotBar,
                                dynamic_id: None,
                            },
                            slots: vec![slot(0, 15, 34), slot(8, 1, 0)],
                        },
                        ItemStackResponseContainerInfo {
                            full_container_name: FullContainerName {
                                container_name: ContainerName::Cursor,
                                dynamic_id: Some(7),
                            },
                            slots: vec![slot(0, 1, 36)],
                        },
                    ],
                },
                // A refused request carries no containers.
                ItemStackResponseInfo {
                    result: 1,
                    client_request_id: VarInt(-5),
                    containers: Vec::new(),
                },
            ],
        };
        let mut first = Vec::new();
        packet.write(&mut first).unwrap();
        let mut reader = first.as_slice();
        let read = CItemStackResponse::read(&mut reader).unwrap();
        assert!(reader.is_empty(), "{} bytes left over", reader.len());

        let accepted = &read.responses[0];
        assert_eq!(
            (accepted.result, accepted.client_request_id),
            (0, VarInt(-3))
        );
        let hotbar = &accepted.containers[0];
        assert_eq!(
            hotbar.full_container_name.container_name,
            ContainerName::HotBar
        );
        assert_eq!(hotbar.slots[0].item_stack_net_id, VarInt(34));
        assert_eq!(hotbar.slots[0].amount, 15);
        assert_eq!(hotbar.slots[1].item_stack_net_id, VarInt(0));
        assert_eq!(hotbar.slots[1].durability_correction, VarInt(-3));
        assert_eq!(
            accepted.containers[1].full_container_name.dynamic_id,
            Some(7)
        );
        assert_eq!(read.responses[1].result, 1);
        assert!(read.responses[1].containers.is_empty());

        let mut second = Vec::new();
        read.write(&mut second).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn rejects_too_many_responses() {
        let mut bytes = Vec::new();
        VarUInt(MAX_RESPONSES + 1).write(&mut bytes).unwrap();
        let error = CItemStackResponse::read(&mut bytes.as_slice()).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidData);
    }
}
