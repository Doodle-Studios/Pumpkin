use std::hash::Hash;

use pumpkin_data::{
    BlockState, BlockStateId,
    block_properties::{has_random_ticks, is_air, is_liquid},
    fluid::Fluid,
};
use pumpkin_util::encompassing_bits;
use rustc_hash::FxHashMap;
use tracing::warn;

use super::format::{ChunkSectionBiomes, ChunkSectionBlockStates};

fn bedrock_palette_bits(palette_len: usize) -> u8 {
    match encompassing_bits(palette_len) {
        bits @ 0..=6 => bits,
        7..=8 => 8,
        _ => 16,
    }
}

/// Packs the indices of a 16x16x16 Bedrock storage into little-endian words,
/// `32 / bits_per_entry` per word, in Bedrock's x, z, y order.
/// `index_at(x, y, z)` gives the index for each position.
fn pack_bedrock_storage(
    bits_per_entry: u8,
    index_at: impl Fn(usize, usize, usize) -> u32,
) -> Box<[u32]> {
    let per_word = usize::from(32 / bits_per_entry);
    let mut words = vec![0u32; 4096usize.div_ceil(per_word)];
    let mut i = 0;
    for x in 0..16 {
        for z in 0..16 {
            for y in 0..16 {
                let shift = u32::from(bits_per_entry) * (i % per_word) as u32;
                words[i / per_word] |= index_at(x, y, z) << shift;
                i += 1;
            }
        }
    }
    words.into_boxed_slice()
}

/// 3d array indexed by y,z,x
type AbstractCube<T, const DIM: usize> = [[[T; DIM]; DIM]; DIM];

#[inline]
#[must_use]
pub fn has_random_ticking_fluid(id: BlockStateId) -> bool {
    Fluid::from_state_id(id).is_some_and(|fluid| Fluid::same_fluid_type(fluid.id, Fluid::LAVA.id))
}

/// Returns the block state for Bedrock's secondary water storage.
///
/// Java stores water inside waterlogged and aquatic block states, while Bedrock stores it in a
/// separate subchunk layer. Pure fluid blocks stay in the primary layer.
#[must_use]
pub fn bedrock_water_state(id: BlockStateId) -> BlockStateId {
    let state = BlockState::from_id(id);
    if state.is_waterlogged() || (state.is_liquid() && Fluid::from_state_id(id).is_none()) {
        pumpkin_data::Block::WATER.default_state.id
    } else {
        pumpkin_data::Block::AIR.default_state.id
    }
}

#[derive(Clone)]
pub struct HeterogeneousPaletteData<V: Hash + Eq + Copy, const DIM: usize> {
    storage: PaletteStorage<V, DIM>,
    palette: Vec<V>,
    counts: Vec<u16>,
}

#[derive(Clone)]
enum PaletteStorage<V, const DIM: usize> {
    Dense(Box<AbstractCube<V, DIM>>),
    Indexed(Box<AbstractCube<u8, DIM>>),
}

impl<V: Hash + Eq + Copy + Default, const DIM: usize> HeterogeneousPaletteData<V, DIM> {
    fn get(&self, x: usize, y: usize, z: usize) -> V {
        debug_assert!(x < DIM);
        debug_assert!(y < DIM);
        debug_assert!(z < DIM);

        match &self.storage {
            PaletteStorage::Dense(cube) => cube[y][z][x],
            PaletteStorage::Indexed(indices) => self.palette[indices[y][z][x] as usize],
        }
    }

    /// Returns the Original
    fn set(&mut self, x: usize, y: usize, z: usize, value: V) -> V {
        debug_assert!(x < DIM);
        debug_assert!(y < DIM);
        debug_assert!(z < DIM);

        let original = self.get(x, y, z);
        if original == value {
            return original;
        }

        let original_index = self
            .palette
            .iter()
            .position(|v| v == &original)
            .unwrap_or(0);

        // Find or add the new value to the palette.
        let new_index = if let Some(new_index) = self.palette.iter().position(|v| v == &value) {
            self.counts[new_index] += 1;
            new_index
        } else {
            self.palette.push(value);
            self.counts.push(1);
            self.palette.len() - 1
        };

        // Handle storage upgrades or updates
        let mut upgraded = false;
        match &mut self.storage {
            PaletteStorage::Dense(cube) => {
                cube[y][z][x] = value;
            }
            PaletteStorage::Indexed(indices) => {
                if new_index <= 255 {
                    indices[y][z][x] = new_index as u8;
                } else {
                    // Upgrade to Dense
                    let mut cube = Box::new([[[V::default(); DIM]; DIM]; DIM]);
                    for (i, v) in cube
                        .as_flattened_mut()
                        .as_flattened_mut()
                        .iter_mut()
                        .enumerate()
                    {
                        let y = i / (DIM * DIM);
                        let z = (i / DIM) % DIM;
                        let x = i % DIM;
                        *v = self.palette[indices[y][z][x] as usize];
                    }
                    cube[y][z][x] = value;
                    self.storage = PaletteStorage::Dense(cube);
                    upgraded = true;
                }
            }
        }

        self.counts[original_index] -= 1;

        if self.counts[original_index] == 0 {
            let last_index = self.palette.len() - 1;
            // Remove from palette and counts Vecs if the count hits zero.
            self.palette.swap_remove(original_index);
            self.counts.swap_remove(original_index);

            if self.palette.capacity() > 16 && self.palette.len() < self.palette.capacity() / 2 {
                self.palette.shrink_to_fit();
                self.counts.shrink_to_fit();
            }

            // If we are indexed, we need to update all indices because swap_remove changed indices
            if !upgraded && let PaletteStorage::Indexed(indices) = &mut self.storage {
                for row in indices.iter_mut() {
                    for col in row.iter_mut() {
                        for idx in col.iter_mut() {
                            if *idx as usize == last_index {
                                *idx = original_index as u8;
                            }
                        }
                    }
                }
            }
        }

        original
    }

    /// The palette index Bedrock's conversion writes for each position, in
    /// this container's y, z, x order.
    ///
    /// That's the last palette entry equal to the stored value, which is what
    /// a key-to-index map collected from the palette gives, or 0 for a value
    /// missing from the palette. `Indexed` storage already holds an index, so
    /// it only goes through a per-entry table (which differs from the identity
    /// only if the palette repeats a value), and `Dense` storage hashes each
    /// value with `FxHash`.
    fn network_indices(&self) -> Box<[u16]> {
        let key_to_index: FxHashMap<V, u16> = self
            .palette
            .iter()
            .enumerate()
            .map(|(index, key)| (*key, index as u16))
            .collect();
        match &self.storage {
            PaletteStorage::Indexed(indices) => {
                let per_entry: Vec<u16> = self
                    .palette
                    .iter()
                    .map(|key| key_to_index.get(key).copied().unwrap_or(0))
                    .collect();
                indices
                    .as_flattened()
                    .as_flattened()
                    .iter()
                    .map(|&index| per_entry[usize::from(index)])
                    .collect()
            }
            PaletteStorage::Dense(cube) => cube
                .as_flattened()
                .as_flattened()
                .iter()
                .map(|key| key_to_index.get(key).copied().unwrap_or(0))
                .collect(),
        }
    }
}

/// A paletted container is a cube of registry ids. It uses a custom compression scheme based on how
/// may distinct registry ids are in the cube.
#[derive(Clone)]
pub enum PalettedContainer<V: Hash + Eq + Copy + Default, const DIM: usize> {
    Homogeneous(V),
    Heterogeneous(Box<HeterogeneousPaletteData<V, DIM>>),
}

impl<V: Hash + Eq + Copy + Default, const DIM: usize> PalettedContainer<V, DIM> {
    pub const SIZE: usize = DIM;
    pub const VOLUME: usize = DIM * DIM * DIM;

    fn from_cube(cube: Box<AbstractCube<V, DIM>>) -> Self {
        let mut palette: Vec<V> = Vec::new();
        let mut counts: Vec<u16> = Vec::new();

        // Iterate over the flattened cube to populate the palette and counts
        for val in cube.as_flattened().as_flattened() {
            if let Some(index) = palette.iter().position(|v| v == val) {
                // Value already exists, increment its count
                counts[index] += 1;
            } else {
                // New value, add it to the palette and start its count
                palette.push(*val);
                counts.push(1);
            }
        }

        if palette.len() == 1 {
            // Fast path: the cube is homogeneous, so we can store just one value
            Self::Homogeneous(palette[0])
        } else {
            // Heterogeneous cube, store the full data
            if palette.len() <= 256 && std::mem::size_of::<V>() > 1 {
                let mut indices = Box::new([[[0u8; DIM]; DIM]; DIM]);
                for (i, v) in cube.as_flattened().as_flattened().iter().enumerate() {
                    let idx = palette.iter().position(|p| p == v).unwrap_or(0);
                    indices.as_flattened_mut().as_flattened_mut()[i] = idx as u8;
                }
                Self::Heterogeneous(Box::new(HeterogeneousPaletteData {
                    storage: PaletteStorage::Indexed(indices),
                    palette,
                    counts,
                }))
            } else {
                Self::Heterogeneous(Box::new(HeterogeneousPaletteData {
                    storage: PaletteStorage::Dense(cube),
                    palette,
                    counts,
                }))
            }
        }
    }

    /// Builds a complete palette without paying the bookkeeping cost of individual mutations.
    pub(crate) fn from_fn(mut value_at: impl FnMut(usize, usize, usize) -> V) -> Self {
        let mut cube = Box::new([[[V::default(); DIM]; DIM]; DIM]);
        let mut indices = Box::new([[[0u8; DIM]; DIM]; DIM]);
        let mut palette = Vec::new();
        let mut counts = Vec::<u16>::new();

        for y in 0..DIM {
            for z in 0..DIM {
                for x in 0..DIM {
                    let value = value_at(x, y, z);
                    cube[y][z][x] = value;
                    let index =
                        if let Some(index) = palette.iter().position(|entry| *entry == value) {
                            counts[index] += 1;
                            index
                        } else {
                            palette.push(value);
                            counts.push(1);
                            palette.len() - 1
                        };
                    if let Ok(index) = u8::try_from(index) {
                        indices[y][z][x] = index;
                    }
                }
            }
        }

        if palette.len() == 1 {
            return Self::Homogeneous(palette[0]);
        }

        let storage = if palette.len() <= 256 && std::mem::size_of::<V>() > 1 {
            PaletteStorage::Indexed(indices)
        } else {
            PaletteStorage::Dense(cube)
        };
        Self::Heterogeneous(Box::new(HeterogeneousPaletteData {
            storage,
            palette,
            counts,
        }))
    }

    fn bits_per_entry(&self) -> u8 {
        match self {
            Self::Homogeneous(_) => 0,
            Self::Heterogeneous(data) => encompassing_bits(data.counts.len()),
        }
    }

    pub fn to_palette_and_packed_data(&self, bits_per_entry: u8) -> (Box<[V]>, Box<[i64]>) {
        match self {
            Self::Homogeneous(registry_id) => (Box::new([*registry_id]), Box::new([])),
            Self::Heterogeneous(data) => {
                debug_assert!(bits_per_entry >= encompassing_bits(data.counts.len()));
                debug_assert!(bits_per_entry <= 15);

                // Don't use HashMap's here, because its slow
                let blocks_per_i64 = 64 / bits_per_entry;

                let packed_indices: Box<[i64]> = match &data.storage {
                    PaletteStorage::Dense(cube) => cube
                        .as_flattened()
                        .as_flattened()
                        .chunks(blocks_per_i64 as usize)
                        .map(|chunk| {
                            chunk.iter().enumerate().fold(0, |acc, (index, key)| {
                                let key_index =
                                    data.palette.iter().position(|&x| x == *key).unwrap_or(0);
                                debug_assert!((1 << bits_per_entry) > key_index);

                                let packed_offset_index =
                                    (key_index as u64) << (bits_per_entry as u64 * index as u64);
                                acc | packed_offset_index as i64
                            })
                        })
                        .collect(),
                    PaletteStorage::Indexed(indices) => indices
                        .as_flattened()
                        .as_flattened()
                        .chunks(blocks_per_i64 as usize)
                        .map(|chunk| {
                            chunk.iter().enumerate().fold(0, |acc, (index, key_index)| {
                                let key_index = *key_index as usize;
                                debug_assert!((1 << bits_per_entry) > key_index);

                                let packed_offset_index =
                                    (key_index as u64) << (bits_per_entry as u64 * index as u64);
                                acc | packed_offset_index as i64
                            })
                        })
                        .collect(),
                };

                (data.palette.clone().into_boxed_slice(), packed_indices)
            }
        }
    }

    #[must_use]
    pub fn from_palette_and_packed_data(
        palette: &[V],
        packed_data: &[i64],
        minimum_bits_per_entry: u8,
    ) -> Self {
        if palette.is_empty() {
            warn!("No palette data! Defaulting...");
            return Self::Homogeneous(V::default());
        }

        if palette.len() == 1 {
            return Self::Homogeneous(palette[0]);
        }

        let bits_per_key = encompassing_bits(palette.len()).max(minimum_bits_per_entry);
        let index_mask = (1 << bits_per_key) - 1;
        let keys_per_i64 = 64 / bits_per_key;

        // Optimized path for indexed storage if palette is small enough
        if palette.len() <= 256 && std::mem::size_of::<V>() > 1 {
            let mut indices = Box::new([[[0u8; DIM]; DIM]; DIM]);
            let mut counts = vec![0u16; palette.len()];
            let indices_flat = indices.as_flattened_mut().as_flattened_mut();

            let mut packed_data_iter = packed_data.iter();
            let mut current_packed_word = *packed_data_iter.next().unwrap_or(&0);

            for (i, index_out) in indices_flat.iter_mut().enumerate().take(Self::VOLUME) {
                let bit_index_in_word = i % keys_per_i64 as usize;
                if bit_index_in_word == 0 && i > 0 {
                    current_packed_word = *packed_data_iter.next().unwrap_or(&0);
                }

                let lookup_index = ((current_packed_word as u64)
                    >> (bit_index_in_word as u64 * bits_per_key as u64))
                    & index_mask;

                let idx = lookup_index as usize;
                if idx < palette.len() {
                    *index_out = idx as u8;
                    counts[idx] += 1;
                } else {
                    warn!("Lookup index out of bounds! Defaulting...");
                    // value is already 0, and counts[0] will be updated correctly if we track it
                }
            }
            // fix counts[0] if it was skipped in out-of-bounds cases (rare)
            // But actually we should just ensure it's correct.

            return Self::Heterogeneous(Box::new(HeterogeneousPaletteData {
                storage: PaletteStorage::Indexed(indices),
                palette: palette.to_vec(),
                counts,
            }));
        }

        let mut decompressed_values = Vec::with_capacity(Self::VOLUME);

        let mut packed_data_iter = packed_data.iter();
        let mut current_packed_word = *packed_data_iter.next().unwrap_or(&0);

        for i in 0..Self::VOLUME {
            let bit_index_in_word = i % keys_per_i64 as usize;

            if bit_index_in_word == 0 && i > 0 {
                current_packed_word = *packed_data_iter.next().unwrap_or(&0);
            }

            let lookup_index = (current_packed_word as u64
                >> (bit_index_in_word as u64 * bits_per_key as u64))
                & index_mask;

            let value = palette
                .get(lookup_index as usize)
                .copied()
                .unwrap_or_else(|| {
                    warn!("Lookup index out of bounds! Defaulting...");
                    V::default()
                });

            decompressed_values.push(value);
        }

        // Now, with all decompressed values, build the counts.
        let mut counts = vec![0; palette.len()];

        for &value in &decompressed_values {
            // This is the key optimization: find the index in the palette Vec
            // and increment the corresponding count.
            if let Some(index) = palette.iter().position(|v| v == &value) {
                counts[index] += 1;
            } else {
                // This case should ideally not happen if the palette is complete.
                warn!("Decompressed value not found in palette!");
            }
        }

        let mut cube = Box::new([[[V::default(); DIM]; DIM]; DIM]);
        cube.as_flattened_mut()
            .as_flattened_mut()
            .copy_from_slice(&decompressed_values);

        Self::Heterogeneous(Box::new(HeterogeneousPaletteData {
            storage: PaletteStorage::Dense(cube),
            palette: palette.to_vec(),
            counts,
        }))
    }

    pub fn get(&self, x: usize, y: usize, z: usize) -> V {
        match self {
            Self::Homogeneous(value) => *value,
            Self::Heterogeneous(data) => data.get(x, y, z),
        }
    }

    pub fn set(&mut self, x: usize, y: usize, z: usize, value: V) -> V {
        debug_assert!(x < Self::SIZE);
        debug_assert!(y < Self::SIZE);
        debug_assert!(z < Self::SIZE);

        match self {
            Self::Homogeneous(original) => {
                let original = *original;
                if value != original {
                    let mut cube = Box::new([[[original; DIM]; DIM]; DIM]);
                    cube[y][z][x] = value;
                    *self = Self::from_cube(cube);
                }
                original
            }
            Self::Heterogeneous(data) => {
                let original = data.set(x, y, z, value);
                if data.counts.len() == 1 {
                    *self = Self::Homogeneous(data.palette[0]);
                }
                original
            }
        }
    }

    pub fn iter(&self) -> Box<dyn Iterator<Item = V> + '_> {
        match self {
            Self::Homogeneous(registry_id) => {
                Box::new(std::iter::repeat_n(*registry_id, Self::VOLUME))
            }
            Self::Heterogeneous(data) => match &data.storage {
                PaletteStorage::Dense(cube) => {
                    Box::new(cube.as_flattened().as_flattened().iter().copied())
                }
                PaletteStorage::Indexed(indices) => Box::new(
                    indices
                        .as_flattened()
                        .as_flattened()
                        .iter()
                        .map(|&idx| data.palette[idx as usize]),
                ),
            },
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Self::Homogeneous(value) => *value == V::default(),
            Self::Heterogeneous(_) => false,
        }
    }
}

impl<'a, V: Hash + Eq + Copy + Default, const DIM: usize> IntoIterator
    for &'a PalettedContainer<V, DIM>
{
    type Item = V;
    type IntoIter = Box<dyn Iterator<Item = Self::Item> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            PalettedContainer::Homogeneous(registry_id) => Box::new(std::iter::repeat_n(
                *registry_id,
                PalettedContainer::<V, DIM>::VOLUME,
            )),
            PalettedContainer::Heterogeneous(data) => match &data.storage {
                PaletteStorage::Dense(cube) => {
                    Box::new(cube.as_flattened().as_flattened().iter().copied())
                }
                PaletteStorage::Indexed(indices) => Box::new(
                    indices
                        .as_flattened()
                        .as_flattened()
                        .iter()
                        .map(|&idx| data.palette[idx as usize]),
                ),
            },
        }
    }
}

impl<V: Default + Hash + Eq + Copy, const DIM: usize> Default for PalettedContainer<V, DIM> {
    fn default() -> Self {
        Self::Homogeneous(V::default())
    }
}

impl BiomePalette {
    #[must_use]
    pub fn convert_network(&self) -> NetworkSerialization<u8> {
        match self {
            Self::Homogeneous(registry_id) => NetworkSerialization {
                bits_per_entry: 0,
                palette: NetworkPalette::Single(*registry_id),
                packed_data: Box::new([]),
            },
            Self::Heterogeneous(data) => {
                let raw_bits_per_entry = encompassing_bits(data.counts.len());
                if raw_bits_per_entry > BIOME_NETWORK_MAX_MAP_BITS {
                    let bits_per_entry = BIOME_NETWORK_MAX_BITS;
                    let values_per_i64 = 64 / bits_per_entry;
                    let mut packed_data =
                        Vec::with_capacity(Self::VOLUME.div_ceil(values_per_i64 as usize));
                    let mut current_idx = 0;
                    while current_idx < Self::VOLUME {
                        let mut acc = 0u64;
                        for i in 0..values_per_i64 as usize {
                            if current_idx + i < Self::VOLUME {
                                let y = (current_idx + i) / (Self::SIZE * Self::SIZE);
                                let z = ((current_idx + i) / Self::SIZE) % Self::SIZE;
                                let x = (current_idx + i) % Self::SIZE;
                                let value = data.get(x, y, z);
                                debug_assert!((1 << bits_per_entry) > value);
                                acc |= (value as u64) << (bits_per_entry as u64 * i as u64);
                            }
                        }
                        packed_data.push(acc as i64);
                        current_idx += values_per_i64 as usize;
                    }

                    NetworkSerialization {
                        bits_per_entry,
                        palette: NetworkPalette::Direct,
                        packed_data: packed_data.into_boxed_slice(),
                    }
                } else {
                    let bits_per_entry = raw_bits_per_entry.max(BIOME_NETWORK_MIN_MAP_BITS);
                    let (palette, packed) = self.to_palette_and_packed_data(bits_per_entry);

                    NetworkSerialization {
                        bits_per_entry,
                        palette: NetworkPalette::Indirect(palette),
                        packed_data: packed,
                    }
                }
            }
        }
    }

    #[must_use]
    pub fn convert_be_network(&self) -> BeNetworkSerialization<u8> {
        match self {
            Self::Homogeneous(registry_id) => BeNetworkSerialization {
                bits_per_entry: 0,
                palette: NetworkPalette::Single(
                    pumpkin_data::biome::Biome::from_id(*registry_id)
                        .map_or(1, |b| b.be_network_id),
                ),
                packed_data: Box::new([]),
            },
            Self::Heterogeneous(data) => {
                let bits_per_entry = bedrock_palette_bits(data.palette.len());

                // One index per 4x4x4 biome cell, spread over its 64 blocks.
                let indices = data.network_indices();
                let packed_data = pack_bedrock_storage(bits_per_entry, |x, y, z| {
                    u32::from(indices[((y / 4) * 4 + z / 4) * 4 + x / 4])
                });

                BeNetworkSerialization {
                    bits_per_entry,
                    palette: NetworkPalette::Indirect(
                        data.palette
                            .iter()
                            .map(|&id| {
                                pumpkin_data::biome::Biome::from_id(id)
                                    .map_or(1, |b| b.be_network_id)
                            })
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    ),
                    packed_data,
                }
            }
        }
    }

    #[must_use]
    pub fn from_disk_nbt(nbt: ChunkSectionBiomes) -> Self {
        let palette = nbt.palette;

        Self::from_palette_and_packed_data(
            &palette,
            nbt.data.as_ref().unwrap_or(&Box::default()),
            BIOME_DISK_MIN_BITS,
        )
    }

    #[must_use]
    pub fn to_disk_nbt(&self) -> ChunkSectionBiomes {
        #[expect(clippy::unnecessary_min_or_max)]
        let bits_per_entry = self.bits_per_entry().max(BIOME_DISK_MIN_BITS);
        let (palette, packed_data) = self.to_palette_and_packed_data(bits_per_entry);
        ChunkSectionBiomes {
            data: if packed_data.is_empty() {
                None
            } else {
                Some(packed_data)
            },
            palette,
        }
    }
}

impl BlockPalette {
    /// Builds Bedrock's secondary water storage for blocks whose Java state also contains water.
    ///
    /// Bedrock expects mixed secondary storages to use a one-bit, air-first palette. The first
    /// entry is the default for every position not occupied by water.
    #[must_use]
    pub fn convert_be_water_network(&self) -> Option<BeNetworkSerialization<u32>> {
        let air = pumpkin_data::Block::AIR.default_state.id;
        let water = pumpkin_data::Block::WATER.default_state.id;
        match self {
            Self::Homogeneous(id) => {
                (bedrock_water_state(*id) != air).then_some(BeNetworkSerialization {
                    bits_per_entry: 0,
                    palette: NetworkPalette::Single(BlockState::to_be_network_id(water)),
                    packed_data: Box::new([]),
                })
            }
            Self::Heterogeneous(data) => {
                let water_states = data
                    .palette
                    .iter()
                    .map(|&id| bedrock_water_state(id) != air)
                    .collect::<Vec<_>>();
                if !water_states.iter().any(|&waterlogged| waterlogged) {
                    return None;
                }

                let indices = data.network_indices();
                let packed_data = pack_bedrock_storage(1, |x, y, z| {
                    u32::from(water_states[usize::from(indices[(y * 16 + z) * 16 + x])])
                });

                Some(BeNetworkSerialization {
                    bits_per_entry: 1,
                    palette: NetworkPalette::Indirect(Box::new([
                        BlockState::to_be_network_id(air),
                        BlockState::to_be_network_id(water),
                    ])),
                    packed_data,
                })
            }
        }
    }

    #[must_use]
    pub fn convert_network(&self) -> NetworkSerialization<u16> {
        match self {
            Self::Homogeneous(registry_id) => NetworkSerialization {
                bits_per_entry: 0,
                palette: NetworkPalette::Single(registry_id.as_u16()),
                packed_data: Box::new([]),
            },
            Self::Heterogeneous(data) => {
                let raw_bits_per_entry = encompassing_bits(data.counts.len());
                if raw_bits_per_entry > BLOCK_NETWORK_MAX_MAP_BITS {
                    let bits_per_entry = BLOCK_NETWORK_MAX_BITS;
                    let values_per_i64 = 64 / bits_per_entry;
                    let mut packed_data =
                        Vec::with_capacity(Self::VOLUME.div_ceil(values_per_i64 as usize));
                    let mut current_idx = 0;
                    while current_idx < Self::VOLUME {
                        let mut acc = 0u64;
                        for i in 0..values_per_i64 as usize {
                            if current_idx + i < Self::VOLUME {
                                let y = (current_idx + i) / (Self::SIZE * Self::SIZE);
                                let z = ((current_idx + i) / Self::SIZE) % Self::SIZE;
                                let x = (current_idx + i) % Self::SIZE;
                                let value = data.get(x, y, z).as_u16();
                                debug_assert!((1u32 << bits_per_entry) > u32::from(value));
                                acc |= (value as u64) << (bits_per_entry as u64 * i as u64);
                            }
                        }
                        packed_data.push(acc as i64);
                        current_idx += values_per_i64 as usize;
                    }

                    NetworkSerialization {
                        bits_per_entry,
                        palette: NetworkPalette::Direct,
                        packed_data: packed_data.into_boxed_slice(),
                    }
                } else {
                    let bits_per_entry = raw_bits_per_entry.max(BLOCK_NETWORK_MIN_MAP_BITS);
                    let (palette, packed) = self.to_palette_and_packed_data(bits_per_entry);

                    NetworkSerialization {
                        bits_per_entry,
                        palette: NetworkPalette::Indirect(
                            palette.iter().map(|v| v.as_u16()).collect(),
                        ),
                        packed_data: packed,
                    }
                }
            }
        }
    }

    #[must_use]
    pub fn convert_be_network(&self) -> BeNetworkSerialization<u32> {
        match self {
            Self::Homogeneous(registry_id) => BeNetworkSerialization {
                bits_per_entry: 0,
                palette: NetworkPalette::Single(BlockState::to_be_network_id(*registry_id)),
                packed_data: Box::new([]),
            },
            Self::Heterogeneous(data) => {
                let bits_per_entry = bedrock_palette_bits(data.palette.len());

                // Java stores y, z, x; `pack_bedrock_storage` walks Bedrock's
                // x, z, y. Please test your code on bedrock before merging.
                let indices = data.network_indices();
                let packed_data = pack_bedrock_storage(bits_per_entry, |x, y, z| {
                    u32::from(indices[(y * 16 + z) * 16 + x])
                });

                BeNetworkSerialization {
                    bits_per_entry,
                    palette: NetworkPalette::Indirect(
                        data.palette
                            .iter()
                            .map(|&id| BlockState::to_be_network_id(id))
                            .collect(),
                    ),
                    packed_data,
                }
            }
        }
    }

    /// Check if the entire chunk is filled with only air
    #[must_use]
    pub fn has_only_air(&self) -> bool {
        match self {
            Self::Homogeneous(id) => is_air(*id),
            Self::Heterogeneous(data) => data.palette.iter().all(|&id| is_air(id)),
        }
    }

    #[must_use]
    pub fn random_ticking_counts(&self) -> (u16, u16) {
        match self {
            Self::Homogeneous(registry_id) => {
                let block_count = if has_random_ticks(*registry_id) {
                    Self::VOLUME as u16
                } else {
                    0
                };
                let fluid_count = if has_random_ticking_fluid(*registry_id) {
                    Self::VOLUME as u16
                } else {
                    0
                };
                (block_count, fluid_count)
            }
            Self::Heterogeneous(data) => data.palette.iter().zip(data.counts.iter()).fold(
                (0, 0),
                |(block_count, fluid_count), (registry_id, count)| {
                    let block_count = if has_random_ticks(*registry_id) {
                        block_count.saturating_add(*count)
                    } else {
                        block_count
                    };
                    let fluid_count = if has_random_ticking_fluid(*registry_id) {
                        fluid_count.saturating_add(*count)
                    } else {
                        fluid_count
                    };
                    (block_count, fluid_count)
                },
            ),
        }
    }

    #[must_use]
    pub fn non_air_block_count(&self) -> u16 {
        match self {
            Self::Homogeneous(registry_id) => {
                if is_air(*registry_id) {
                    0
                } else {
                    Self::VOLUME as u16
                }
            }
            Self::Heterogeneous(data) => data
                .palette
                .iter()
                .zip(data.counts.iter())
                .filter_map(|(registry_id, count)| (!is_air(*registry_id)).then_some(*count))
                .sum(),
        }
    }

    #[must_use]
    pub fn liquid_block_count(&self) -> u16 {
        match self {
            Self::Homogeneous(registry_id) => {
                if is_liquid(*registry_id) {
                    Self::VOLUME as u16
                } else {
                    0
                }
            }
            Self::Heterogeneous(data) => data
                .palette
                .iter()
                .zip(data.counts.iter())
                .filter_map(|(registry_id, count)| is_liquid(*registry_id).then_some(*count))
                .sum(),
        }
    }

    #[must_use]
    pub fn from_disk_nbt(nbt: ChunkSectionBlockStates) -> Self {
        let palette = nbt.palette;

        Self::from_palette_and_packed_data(
            &palette,
            nbt.data.as_ref().unwrap_or(&Box::default()),
            BLOCK_DISK_MIN_BITS,
        )
    }

    #[must_use]
    pub fn to_disk_nbt(&self) -> ChunkSectionBlockStates {
        let bits_per_entry = self.bits_per_entry().max(BLOCK_DISK_MIN_BITS);
        let (palette, packed_data) = self.to_palette_and_packed_data(bits_per_entry);
        ChunkSectionBlockStates {
            data: if packed_data.is_empty() {
                None
            } else {
                Some(packed_data)
            },
            palette,
        }
    }

    pub(crate) fn write_pump_binary(&self, out: &mut Vec<u8>) {
        use bytes::BufMut;

        match self {
            Self::Homogeneous(id) => {
                out.put_u8(0);
                out.put_u16(id.as_u16());
            }
            Self::Heterogeneous(data) => match &data.storage {
                PaletteStorage::Indexed(indices) => {
                    out.put_u8(1);
                    out.put_u16(data.palette.len() as u16);
                    for (&id, &count) in data.palette.iter().zip(&data.counts) {
                        out.put_u16(id.as_u16());
                        out.put_u16(count);
                    }
                    out.extend_from_slice(indices.as_flattened().as_flattened());
                }
                PaletteStorage::Dense(cube) => {
                    out.put_u8(2);
                    for &id in cube.as_flattened().as_flattened() {
                        out.put_u16(id.as_u16());
                    }
                }
            },
        }
    }

    pub(crate) fn read_pump_binary(buf: &mut &[u8]) -> Result<Self, super::ChunkParsingError> {
        use bytes::Buf;

        if !buf.has_remaining() {
            return Err(super::ChunkParsingError::ErrorDeserializingChunk(
                "Unexpected EOF reading block palette mode".into(),
            ));
        }
        match buf.get_u8() {
            0 => {
                if buf.remaining() < 2 {
                    return Err(super::ChunkParsingError::ErrorDeserializingChunk(
                        "Unexpected EOF reading homogeneous block palette".into(),
                    ));
                }
                Ok(Self::Homogeneous(BlockStateId::new_or_air(buf.get_u16())))
            }
            1 => {
                if buf.remaining() < 2 {
                    return Err(super::ChunkParsingError::ErrorDeserializingChunk(
                        "Unexpected EOF reading indexed block palette length".into(),
                    ));
                }
                let palette_len = buf.get_u16() as usize;
                if !(1..=256).contains(&palette_len)
                    || buf.remaining() < palette_len * 4 + Self::VOLUME
                {
                    return Err(super::ChunkParsingError::ErrorDeserializingChunk(
                        "Invalid indexed block palette length".into(),
                    ));
                }
                let mut palette = Vec::with_capacity(palette_len);
                let mut counts = Vec::with_capacity(palette_len);
                let mut count_sum = 0u32;
                for _ in 0..palette_len {
                    palette.push(BlockStateId::new_or_air(buf.get_u16()));
                    let c = buf.get_u16();
                    counts.push(c);
                    count_sum += u32::from(c);
                }
                if palette_len == 1 {
                    buf.advance(Self::VOLUME);
                    return Ok(Self::Homogeneous(palette[0]));
                }
                let mut indices = Box::new([[[0u8; Self::SIZE]; Self::SIZE]; Self::SIZE]);
                let flat = indices.as_flattened_mut().as_flattened_mut();
                flat.copy_from_slice(&buf[..Self::VOLUME]);
                buf.advance(Self::VOLUME);

                let max_idx = flat.iter().copied().max().unwrap_or(0) as usize;
                if count_sum != Self::VOLUME as u32 || max_idx >= palette_len {
                    counts.fill(0);
                    for idx in flat.iter_mut() {
                        let i = *idx as usize;
                        if i < palette_len {
                            counts[i] += 1;
                        } else {
                            *idx = 0;
                            counts[0] += 1;
                        }
                    }
                }
                Ok(Self::Heterogeneous(Box::new(HeterogeneousPaletteData {
                    storage: PaletteStorage::Indexed(indices),
                    palette,
                    counts,
                })))
            }
            2 => {
                if buf.remaining() < Self::VOLUME * 2 {
                    return Err(super::ChunkParsingError::ErrorDeserializingChunk(
                        "Unexpected EOF reading dense block palette".into(),
                    ));
                }
                let mut cube =
                    Box::new([[[BlockStateId::AIR; Self::SIZE]; Self::SIZE]; Self::SIZE]);
                for slot in cube.as_flattened_mut().as_flattened_mut() {
                    *slot = BlockStateId::new_or_air(buf.get_u16());
                }
                Ok(Self::from_cube(cube))
            }
            mode => Err(super::ChunkParsingError::ErrorDeserializingChunk(format!(
                "Unknown block palette mode {mode}"
            ))),
        }
    }
}

impl BiomePalette {
    pub(crate) fn write_pump_binary(&self, out: &mut Vec<u8>) {
        use bytes::BufMut;

        match self {
            Self::Homogeneous(id) => {
                out.put_u8(0);
                out.put_u8(*id);
            }
            Self::Heterogeneous(data) => {
                out.put_u8(1);
                out.put_u8(data.palette.len() as u8);
                for (&id, &count) in data.palette.iter().zip(&data.counts) {
                    out.put_u8(id);
                    out.put_u16(count);
                }
                match &data.storage {
                    PaletteStorage::Dense(cube) => {
                        out.extend_from_slice(cube.as_flattened().as_flattened());
                    }
                    PaletteStorage::Indexed(indices) => {
                        for &idx in indices.as_flattened().as_flattened() {
                            out.put_u8(data.palette.get(idx as usize).copied().unwrap_or(0));
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn read_pump_binary(buf: &mut &[u8]) -> Result<Self, super::ChunkParsingError> {
        use bytes::Buf;

        if !buf.has_remaining() {
            return Err(super::ChunkParsingError::ErrorDeserializingChunk(
                "Unexpected EOF reading biome palette mode".into(),
            ));
        }
        match buf.get_u8() {
            0 => {
                if !buf.has_remaining() {
                    return Err(super::ChunkParsingError::ErrorDeserializingChunk(
                        "Unexpected EOF reading homogeneous biome palette".into(),
                    ));
                }
                Ok(Self::Homogeneous(buf.get_u8()))
            }
            1 => {
                if !buf.has_remaining() {
                    return Err(super::ChunkParsingError::ErrorDeserializingChunk(
                        "Unexpected EOF reading heterogeneous biome palette length".into(),
                    ));
                }
                let palette_len = buf.get_u8() as usize;
                if !(1..=Self::VOLUME).contains(&palette_len)
                    || buf.remaining() < palette_len * 3 + Self::VOLUME
                {
                    return Err(super::ChunkParsingError::ErrorDeserializingChunk(
                        "Invalid heterogeneous biome palette length".into(),
                    ));
                }
                let mut palette = Vec::with_capacity(palette_len);
                let mut counts = Vec::with_capacity(palette_len);
                let mut count_sum = 0u32;
                for _ in 0..palette_len {
                    palette.push(buf.get_u8());
                    let c = buf.get_u16();
                    counts.push(c);
                    count_sum += u32::from(c);
                }
                let mut cube = Box::new([[[0u8; Self::SIZE]; Self::SIZE]; Self::SIZE]);
                cube.as_flattened_mut()
                    .as_flattened_mut()
                    .copy_from_slice(&buf[..Self::VOLUME]);
                buf.advance(Self::VOLUME);
                if palette_len == 1 {
                    return Ok(Self::Homogeneous(palette[0]));
                }
                if count_sum != Self::VOLUME as u32 {
                    return Ok(Self::from_cube(cube));
                }
                Ok(Self::Heterogeneous(Box::new(HeterogeneousPaletteData {
                    storage: PaletteStorage::Dense(cube),
                    palette,
                    counts,
                })))
            }
            mode => Err(super::ChunkParsingError::ErrorDeserializingChunk(format!(
                "Unknown biome palette mode {mode}"
            ))),
        }
    }
}

/// Represents the different types of data encoding used in Minecraft's bit-packed chunk sections.
///
/// Minecraft uses a "Palette" system to compress chunk data. Instead of sending a full
/// 15-bit `BlockState` ID for every block, it sends a smaller index (e.g., 4 bits) that
/// points to a value in these palettes.
pub enum NetworkPalette<V> {
    /// **Single Value Palette (Bits per entry: 0)**
    ///
    /// Used when an entire chunk section (16x16x16) consists of only one type of block or biome.
    /// No data array follows this palette in the network buffer.
    Single(V),
    /// **Indirect Palette (Bits per entry: 1-8 for blocks, 1-3 for biomes)**
    ///
    /// A list of unique values present in the section. The data array contains indices
    /// pointing into this list.
    Indirect(Box<[V]>),
    /// **Direct Palette (Bits per entry: 15+ for blocks, 6+ for biomes)**
    ///
    /// Used when the section is too complex for a small palette. The data array
    /// contains global Registry IDs directly. No palette list is sent.
    Direct,
}

pub struct NetworkSerialization<V> {
    pub bits_per_entry: u8,
    pub palette: NetworkPalette<V>,
    pub packed_data: Box<[i64]>,
}

pub struct BeNetworkSerialization<V> {
    pub bits_per_entry: u8,
    pub palette: NetworkPalette<V>,
    pub packed_data: Box<[u32]>,
}

// According to the wiki, palette serialization for disk and network is different. Disk
// serialization always uses a palette if greater than one entry. Network serialization packs ids
// directly instead of using a palette above a certain bits-per-entry

// TODO: Do our own testing; do we really need to handle network and disk serialization differently?
pub type BlockPalette = PalettedContainer<BlockStateId, 16>;
const BLOCK_DISK_MIN_BITS: u8 = 4;
const BLOCK_NETWORK_MIN_MAP_BITS: u8 = 4;
const BLOCK_NETWORK_MAX_MAP_BITS: u8 = 8;
pub(crate) const BLOCK_NETWORK_MAX_BITS: u8 = 16;

pub type BiomePalette = PalettedContainer<u8, 4>;
const BIOME_DISK_MIN_BITS: u8 = 0;
const BIOME_NETWORK_MIN_MAP_BITS: u8 = 1;
const BIOME_NETWORK_MAX_MAP_BITS: u8 = 3;
pub(crate) const BIOME_NETWORK_MAX_BITS: u8 = 7;

#[cfg(test)]
mod tests {
    use super::{BeNetworkSerialization, BiomePalette, BlockPalette, NetworkPalette};
    use pumpkin_data::{Block, BlockStateId};

    fn network_palette_values(palette: NetworkPalette<u16>) -> Option<Box<[u16]>> {
        match palette {
            NetworkPalette::Single(value) => Some(Box::new([value])),
            NetworkPalette::Indirect(values) => Some(values),
            NetworkPalette::Direct => None,
        }
    }

    fn assert_bulk_matches_mutations(value_at: impl Fn(usize, usize, usize) -> BlockStateId) {
        let mut mutated = BlockPalette::default();
        for y in 0..BlockPalette::SIZE {
            for z in 0..BlockPalette::SIZE {
                for x in 0..BlockPalette::SIZE {
                    mutated.set(x, y, z, value_at(x, y, z));
                }
            }
        }
        let bulk = BlockPalette::from_fn(value_at);

        assert_eq!(
            mutated.iter().collect::<Vec<_>>(),
            bulk.iter().collect::<Vec<_>>()
        );
        assert_eq!(
            mutated.random_ticking_counts(),
            bulk.random_ticking_counts()
        );
        assert_eq!(mutated.non_air_block_count(), bulk.non_air_block_count());
        assert_eq!(mutated.liquid_block_count(), bulk.liquid_block_count());

        let mutated_network = mutated.convert_network();
        let bulk_network = bulk.convert_network();
        assert_eq!(mutated_network.bits_per_entry, bulk_network.bits_per_entry);
        assert_eq!(mutated_network.packed_data, bulk_network.packed_data);
        assert_eq!(
            network_palette_values(mutated_network.palette),
            network_palette_values(bulk_network.palette)
        );
    }

    #[test]
    fn bulk_palette_matches_individual_mutations() {
        let states = [
            Block::AIR.default_state.id,
            Block::STONE.default_state.id,
            Block::WATER.default_state.id,
            Block::LAVA.default_state.id,
        ];
        assert_bulk_matches_mutations(|x, y, z| states[(x + y + z) % states.len()]);
    }

    #[test]
    fn bulk_palette_handles_homogeneous_sections() {
        assert_bulk_matches_mutations(|_, _, _| Block::STONE.default_state.id);
    }

    #[test]
    fn bulk_palette_handles_direct_network_palettes() {
        assert_bulk_matches_mutations(|x, y, z| {
            BlockStateId::new_or_air(((y * 256 + z * 16 + x) % 300) as u16)
        });
    }

    #[test]
    fn bedrock_palette_uses_supported_bit_widths() {
        let palette = BlockPalette::from_fn(|x, y, z| {
            BlockStateId::new(((y * 256 + z * 16 + x) % 65) as u16).unwrap()
        });
        let network = palette.convert_be_network();

        assert_eq!(network.bits_per_entry, 8);
        assert_eq!(network.packed_data.len(), 1024);
        assert!(matches!(network.palette, NetworkPalette::Indirect(values) if values.len() == 65));
    }

    #[test]
    fn bedrock_water_palette_uses_bedrock_block_order() {
        let mut palette = BlockPalette::default();
        palette.set(1, 2, 3, Block::SEAGRASS.default_state.id);

        let network = palette.convert_be_water_network().unwrap();
        let (x, y, z) = (1, 2, 3);
        let bedrock_index = x * 256 + z * 16 + y;

        assert_eq!(network.bits_per_entry, 1);
        assert_eq!(
            network.packed_data[bedrock_index / 32],
            1 << (bedrock_index % 32)
        );
        assert_eq!(
            network
                .packed_data
                .iter()
                .filter(|&&word| word != 0)
                .count(),
            1
        );
    }

    /// The Bedrock conversions as they were before they used
    /// `network_indices`: a `HashMap` lookup per position. Kept as the oracle
    /// the faster code must match byte for byte.
    mod oracle {
        use std::collections::HashMap;

        use pumpkin_data::BlockState;

        use super::super::{
            BeNetworkSerialization, BiomePalette, BlockPalette, NetworkPalette, bedrock_palette_bits,
            bedrock_water_state,
        };

        pub fn block(palette: &BlockPalette) -> BeNetworkSerialization<u32> {
            match palette {
                BlockPalette::Homogeneous(registry_id) => BeNetworkSerialization {
                    bits_per_entry: 0,
                    palette: NetworkPalette::Single(BlockState::to_be_network_id(*registry_id)),
                    packed_data: Box::new([]),
                },
                BlockPalette::Heterogeneous(data) => {
                    let bits_per_entry = bedrock_palette_bits(data.palette.len());
                    let key_to_index_map: HashMap<_, usize> = data
                        .palette
                        .iter()
                        .enumerate()
                        .map(|(index, key)| (*key, index))
                        .collect();
                    let blocks_per_word = 32 / bits_per_entry;
                    let mut packed_data = Vec::new();
                    let mut current_word: u32 = 0;
                    let mut current_index_in_word = 0;
                    for x in 0..16 {
                        for y in 0..16 {
                            for z in 0..16 {
                                let key = data.get(x, z, y);
                                let key_index = key_to_index_map.get(&key).unwrap_or(&0);
                                current_word |= (*key_index as u32)
                                    << (u32::from(bits_per_entry) * current_index_in_word);
                                current_index_in_word += 1;
                                if current_index_in_word == u32::from(blocks_per_word) {
                                    packed_data.push(current_word);
                                    current_word = 0;
                                    current_index_in_word = 0;
                                }
                            }
                        }
                    }
                    if current_index_in_word > 0 {
                        packed_data.push(current_word);
                    }
                    BeNetworkSerialization {
                        bits_per_entry,
                        palette: NetworkPalette::Indirect(
                            data.palette
                                .iter()
                                .map(|&id| BlockState::to_be_network_id(id))
                                .collect(),
                        ),
                        packed_data: packed_data.into_boxed_slice(),
                    }
                }
            }
        }

        pub fn water(palette: &BlockPalette) -> Option<BeNetworkSerialization<u32>> {
            let air = pumpkin_data::Block::AIR.default_state.id;
            let water = pumpkin_data::Block::WATER.default_state.id;
            match palette {
                BlockPalette::Homogeneous(id) => {
                    (bedrock_water_state(*id) != air).then_some(BeNetworkSerialization {
                        bits_per_entry: 0,
                        palette: NetworkPalette::Single(BlockState::to_be_network_id(water)),
                        packed_data: Box::new([]),
                    })
                }
                BlockPalette::Heterogeneous(data) => {
                    let water_states = data
                        .palette
                        .iter()
                        .map(|&id| bedrock_water_state(id) != air)
                        .collect::<Vec<_>>();
                    if !water_states.iter().any(|&waterlogged| waterlogged) {
                        return None;
                    }
                    let key_to_index = data
                        .palette
                        .iter()
                        .enumerate()
                        .map(|(index, &id)| (id, index))
                        .collect::<HashMap<_, _>>();
                    let mut packed_data = vec![0u32; 4096 / u32::BITS as usize];
                    let mut output_index = 0;
                    for x in 0..16 {
                        for y in 0..16 {
                            for z in 0..16 {
                                let palette_index = key_to_index[&data.get(x, z, y)];
                                if water_states[palette_index] {
                                    packed_data[output_index / u32::BITS as usize] |=
                                        1 << (output_index % u32::BITS as usize);
                                }
                                output_index += 1;
                            }
                        }
                    }
                    Some(BeNetworkSerialization {
                        bits_per_entry: 1,
                        palette: NetworkPalette::Indirect(Box::new([
                            BlockState::to_be_network_id(air),
                            BlockState::to_be_network_id(water),
                        ])),
                        packed_data: packed_data.into_boxed_slice(),
                    })
                }
            }
        }

        pub fn biome(palette: &BiomePalette) -> BeNetworkSerialization<u8> {
            match palette {
                BiomePalette::Homogeneous(registry_id) => BeNetworkSerialization {
                    bits_per_entry: 0,
                    palette: NetworkPalette::Single(
                        pumpkin_data::biome::Biome::from_id(*registry_id)
                            .map_or(1, |b| b.be_network_id),
                    ),
                    packed_data: Box::new([]),
                },
                BiomePalette::Heterogeneous(data) => {
                    let bits_per_entry = bedrock_palette_bits(data.palette.len());
                    let key_to_index_map: HashMap<_, usize> = data
                        .palette
                        .iter()
                        .enumerate()
                        .map(|(index, key)| (*key, index))
                        .collect();
                    let biomes_per_word = 32 / bits_per_entry;
                    let mut packed_data = Vec::new();
                    let mut current_word: u32 = 0;
                    let mut current_index_in_word = 0;
                    for x in 0..16 {
                        for y in 0..16 {
                            for z in 0..16 {
                                let key = palette.get(x / 4, z / 4, y / 4);
                                let key_index = key_to_index_map.get(&key).unwrap_or(&0);
                                current_word |= (*key_index as u32)
                                    << (u32::from(bits_per_entry) * current_index_in_word);
                                current_index_in_word += 1;
                                if current_index_in_word == u32::from(biomes_per_word) {
                                    packed_data.push(current_word);
                                    current_word = 0;
                                    current_index_in_word = 0;
                                }
                            }
                        }
                    }
                    if current_index_in_word > 0 {
                        packed_data.push(current_word);
                    }
                    BeNetworkSerialization {
                        bits_per_entry,
                        palette: NetworkPalette::Indirect(
                            data.palette
                                .iter()
                                .map(|&id| {
                                    pumpkin_data::biome::Biome::from_id(id)
                                        .map_or(1, |b| b.be_network_id)
                                })
                                .collect::<Vec<_>>()
                                .into_boxed_slice(),
                        ),
                        packed_data: packed_data.into_boxed_slice(),
                    }
                }
            }
        }
    }

    fn palette_values<V>(palette: NetworkPalette<V>) -> Option<Box<[V]>> {
        match palette {
            NetworkPalette::Single(value) => Some(Box::new([value])),
            NetworkPalette::Indirect(values) => Some(values),
            NetworkPalette::Direct => None,
        }
    }

    fn assert_same<V: PartialEq + std::fmt::Debug>(
        new: BeNetworkSerialization<V>,
        old: BeNetworkSerialization<V>,
        what: &str,
    ) {
        assert_eq!(new.bits_per_entry, old.bits_per_entry, "{what}: width");
        assert_eq!(new.packed_data, old.packed_data, "{what}: words");
        assert_eq!(
            palette_values(new.palette),
            palette_values(old.palette),
            "{what}: palette"
        );
    }

    fn assert_blocks_match_oracle(palette: &BlockPalette, what: &str) {
        assert_same(palette.convert_be_network(), oracle::block(palette), what);
        match (palette.convert_be_water_network(), oracle::water(palette)) {
            (Some(new), Some(old)) => assert_same(new, old, &format!("{what}, water layer")),
            (None, None) => {}
            (new, old) => panic!(
                "{what}: water layer present {} here, {} in the oracle",
                new.is_some(),
                old.is_some()
            ),
        }
    }

    /// A state id that cycles through `count` states starting at `first`.
    fn cycling(first: u16, count: u16) -> impl Fn(usize, usize, usize) -> BlockStateId {
        move |x, y, z| {
            BlockStateId::new(first + ((y * 256 + z * 16 + x) % usize::from(count)) as u16)
                .unwrap()
        }
    }

    #[test]
    fn bedrock_block_conversion_matches_the_oracle() {
        assert_blocks_match_oracle(
            &BlockPalette::from_fn(|_, _, _| Block::STONE.default_state.id),
            "homogeneous stone",
        );
        assert_blocks_match_oracle(
            &BlockPalette::from_fn(|_, _, _| Block::WATER.default_state.id),
            "homogeneous water",
        );
        // 2 to 256 states use `Indexed` storage, 257 and more `Dense`. Ids
        // from 0 cover air, stone, dirt and saplings; from 1000, blocks
        // that can be waterlogged.
        for count in [2, 16, 65, 256, 257, 300] {
            for first in [0, 1000] {
                let palette = BlockPalette::from_fn(cycling(first, count));
                assert_blocks_match_oracle(&palette, &format!("{count} states from {first}"));
            }
        }
    }

    #[test]
    fn bedrock_water_layer_matches_the_oracle() {
        let waterlogged = Block::OAK_STAIRS
            .states
            .iter()
            .map(|state| state.id)
            .find(|&id| super::bedrock_water_state(id) != Block::AIR.default_state.id)
            .unwrap();
        let mix = [
            Block::AIR.default_state.id,
            Block::STONE.default_state.id,
            Block::WATER.default_state.id,
            Block::SEAGRASS.default_state.id,
            Block::KELP.default_state.id,
            waterlogged,
        ];
        let palette = BlockPalette::from_fn(|x, y, z| mix[(x * 7 + y * 3 + z) % mix.len()]);
        assert_blocks_match_oracle(&palette, "water mix");

        // One waterlogged block, then made heterogeneous by mutation, as
        // gameplay does it.
        let mut palette = BlockPalette::default();
        palette.set(3, 9, 12, waterlogged);
        palette.set(4, 9, 12, Block::STONE.default_state.id);
        assert_blocks_match_oracle(&palette, "mutated water mix");
    }

    #[test]
    fn bedrock_conversion_matches_the_oracle_with_a_repeated_palette_entry() {
        // A palette that repeats a value can come from disk. Collecting it
        // into a map keeps the last index, and the new code must too.
        let stone = Block::STONE.default_state.id;
        let dirt = Block::DIRT.default_state.id;
        // Disk packing: 4 bits per index, 16 indices per i64, cycling 0, 1, 2.
        let mut word = 0i64;
        for slot in 0..16 {
            word |= (slot % 3) << (slot * 4);
        }
        let packed = vec![word; 256];
        let palette = BlockPalette::from_palette_and_packed_data(&[stone, dirt, stone], &packed, 4);
        assert_blocks_match_oracle(&palette, "repeated entry, indexed");

        let mut long: Vec<_> = (0..300).map(|n| BlockStateId::new(n).unwrap()).collect();
        long.push(stone);
        let packed: Vec<i64> = (0..820).map(|n| (n * 2_654_435_761) & 0x7fff_ffff).collect();
        let palette = BlockPalette::from_palette_and_packed_data(&long, &packed, 9);
        assert_blocks_match_oracle(&palette, "repeated entry, dense");
    }

    #[test]
    fn bedrock_biome_conversion_matches_the_oracle() {
        let cases: [(&str, BiomePalette); 4] = [
            ("homogeneous", BiomePalette::from_fn(|_, _, _| 5)),
            ("2 biomes", BiomePalette::from_fn(|x, _, _| (x % 2) as u8)),
            ("5 biomes", BiomePalette::from_fn(|x, y, z| ((x + y + z) % 5) as u8 + 10)),
            ("64 biomes", BiomePalette::from_fn(|x, y, z| (y * 16 + z * 4 + x) as u8)),
        ];
        for (what, palette) in cases {
            assert_same(palette.convert_be_network(), oracle::biome(&palette), what);
        }
        // A repeated entry. Disk packing for three entries: 2 bits per index,
        // 32 indices per i64, cycling 0, 1, 2 over the 64 cells.
        let mut word = 0i64;
        for slot in 0..32 {
            word |= (slot % 3) << (slot * 2);
        }
        let palette = BiomePalette::from_palette_and_packed_data(&[3, 7, 3], &[word, word], 0);
        assert_same(
            palette.convert_be_network(),
            oracle::biome(&palette),
            "repeated entry",
        );
    }
}
