use std::io::{Error, ErrorKind, Write};

use crate::{
    bedrock::server::inventory_transaction::{
        InventoryAction, LegacySetItemSlot, SInventoryTransaction, TransactionData,
    },
    codec::var_uint::VarUInt,
    serial::PacketWrite,
};

/// Mirrors `PacketRead for LegacySetItemSlot`: the container id, then a
/// `VarUInt` count and the slots, one byte each.
impl PacketWrite for LegacySetItemSlot {
    fn write<W: Write>(&self, writer: &mut W) -> Result<(), Error> {
        self.container_id.write(writer)?;
        VarUInt(self.slots.len() as u32).write(writer)?;
        for slot in &self.slots {
            slot.write(writer)?;
        }
        Ok(())
    }
}

/// Mirrors `PacketRead for InventoryAction`: the source type; a `bool` and the
/// window id as a byte; a `bool` and the source flags; the slot; then the old
/// and the new item.
impl PacketWrite for InventoryAction {
    fn write<W: Write>(&self, writer: &mut W) -> Result<(), Error> {
        VarUInt(self.source_type).write(writer)?;
        self.window_id.is_some().write(writer)?;
        if let Some(window_id) = self.window_id {
            i8::try_from(window_id)
                .map_err(|_| {
                    Error::new(
                        ErrorKind::InvalidInput,
                        format!("window id {window_id} doesn't fit in a byte"),
                    )
                })?
                .write(writer)?;
        }
        self.source_flags.is_some().write(writer)?;
        if let Some(source_flags) = self.source_flags {
            VarUInt(source_flags).write(writer)?;
        }
        VarUInt(self.inventory_slot).write(writer)?;
        self.old_item.write(writer)?;
        self.new_item.write(writer)
    }
}

/// The transaction type the reader expects in front of each kind of data.
const fn transaction_type(data: &TransactionData) -> u32 {
    match data {
        TransactionData::Normal(_) => 0,
        TransactionData::Mismatch(_) => 1,
        TransactionData::UseItem(_) => 2,
        TransactionData::UseItemOnEntity(_) => 3,
        TransactionData::ReleaseItem(_) => 4,
    }
}

/// Mirrors `PacketRead for SInventoryTransaction`: the legacy request id; a
/// `bool` and, when there are any, the legacy slots; the transaction type; a
/// `VarUInt` count and the actions; then the type's data.
///
/// `has_value` isn't written: the reader works it out from the actions. A
/// transaction type that doesn't match the data is refused, because the
/// server would read the data as the wrong kind.
impl PacketWrite for SInventoryTransaction {
    fn write<W: Write>(&self, writer: &mut W) -> Result<(), Error> {
        if self.transaction_type.0 != transaction_type(&self.transaction_data) {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "transaction type {} doesn't match its data",
                    self.transaction_type.0
                ),
            ));
        }
        self.legacy_request_id.write(writer)?;
        let has_legacy_slots = !self.legacy_set_item_slots.is_empty();
        has_legacy_slots.write(writer)?;
        if has_legacy_slots {
            VarUInt(self.legacy_set_item_slots.len() as u32).write(writer)?;
            for slots in &self.legacy_set_item_slots {
                slots.write(writer)?;
            }
        }
        self.transaction_type.write(writer)?;
        VarUInt(self.actions.len() as u32).write(writer)?;
        for action in &self.actions {
            action.write(writer)?;
        }
        match &self.transaction_data {
            TransactionData::Normal(data) => data.write(writer),
            TransactionData::Mismatch(data) => data.write(writer),
            TransactionData::UseItem(data) => data.write(writer),
            TransactionData::UseItemOnEntity(data) => data.write(writer),
            TransactionData::ReleaseItem(data) => data.write(writer),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        bedrock::server::inventory_transaction::{HandSlot, NormalTransactionData},
        codec::var_int::VarInt,
        serial::PacketRead,
    };

    /// Reads a capture with Pumpkin's reader, then writes it back with ours.
    fn golden(fixture: &[u8]) -> SInventoryTransaction {
        let mut reader = fixture;
        let packet = SInventoryTransaction::read(&mut reader).unwrap();
        assert!(reader.is_empty(), "{} bytes left over", reader.len());

        let mut written = Vec::new();
        packet.write(&mut written).unwrap();
        assert_eq!(written, fixture);
        packet
    }

    fn use_item(
        packet: &SInventoryTransaction,
    ) -> &crate::bedrock::server::inventory_transaction::UseItemTransactionData {
        let TransactionData::UseItem(data) = &packet.transaction_data else {
            panic!("expected a use-item transaction");
        };
        data
    }

    #[test]
    fn placing_on_top_matches_the_official_client() {
        let packet = golden(include_bytes!(
            "fixtures/inventory_transaction-place_on_top.bin"
        ));
        assert_eq!(packet.legacy_request_id, VarInt(-4));
        assert_eq!(packet.legacy_set_item_slots[0].container_id, 29);
        assert_eq!(packet.legacy_set_item_slots[0].slots, [0]);
        // The client's prediction for hotbar slot 0: dirt, 16 to 15.
        assert_eq!(packet.actions.len(), 1);
        assert_eq!(packet.actions[0].old_item.stack_size, 16);
        assert_eq!(packet.actions[0].new_item.stack_size, 15);
        let data = use_item(&packet);
        assert_eq!(data.action_type, VarInt(0));
        assert_eq!((data.trigger_type, data.client_prediction), (1, 1));
        assert_eq!(data.block_face, 1);
        assert_eq!(data.hand, HandSlot::Mainhand);
    }

    #[test]
    fn the_click_after_a_placement_matches_the_official_client() {
        let packet = golden(include_bytes!(
            "fixtures/inventory_transaction-click_air.bin"
        ));
        assert_eq!(packet.legacy_request_id, VarInt(0));
        assert!(packet.legacy_set_item_slots.is_empty());
        assert!(packet.actions.is_empty());
        let data = use_item(&packet);
        assert_eq!(data.action_type, VarInt(1));
        assert_eq!(data.block_face, 255);
    }

    #[test]
    fn placing_on_a_side_matches_the_official_client() {
        let packet = golden(include_bytes!(
            "fixtures/inventory_transaction-place_on_side.bin"
        ));
        let data = use_item(&packet);
        assert_eq!(data.block_face, 4);
        assert_eq!(data.hot_bar_slot, VarInt(2));
    }

    #[test]
    fn a_drop_matches_the_official_client() {
        let packet = golden(include_bytes!("fixtures/inventory_transaction-drop.bin"));
        assert!(matches!(
            packet.transaction_data,
            TransactionData::Normal(_)
        ));
        assert_eq!(packet.actions.len(), 2);
        // The second action is the drop into the world (source type 2).
        assert_eq!(packet.actions[1].source_type, 2);
        assert_eq!(packet.actions[1].source_flags, Some(0));
    }

    #[test]
    fn a_mismatched_type_is_refused() {
        let packet = SInventoryTransaction {
            legacy_request_id: VarInt(0),
            legacy_set_item_slots: Vec::new(),
            has_value: false,
            actions: Vec::new(),
            transaction_type: VarUInt(2),
            transaction_data: TransactionData::Normal(NormalTransactionData),
        };
        let error = packet.write(&mut Vec::new()).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
    }
}
