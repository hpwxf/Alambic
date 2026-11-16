//! The module's persistent settings, and how they are kept (journal/D-026,
//! D-028).
//!
//! [`Settings`] gathers everything the system settings screen edits: the
//! calibration, the applet to start in, the screen and the encoder
//! directions. It is encoded with `postcard` behind a format byte, and kept
//! in flash by `sequential-storage`'s map over any `embedded-storage`
//! [`NorFlash`]: wear levelling, CRCs and repair after a power cut mid-write
//! come from there, not from code of our own.
//!
//! Every backend provides the flash (the Teensy's own, a file, a buffer) but
//! loads and saves through [`SettingsStore`], so the format and the fallback
//! to defaults are the same everywhere. VCV Rack keeps the settings in the
//! patch instead of a flash image, through [`encode`] and [`decode`], which
//! share the same format byte.
//!
//! Saving erases flash sectors and blocks for tens of milliseconds; it only
//! ever happens on an explicit request from the settings screen, outputs at
//! rest.

use core::ops::Range;

use embedded_storage_async::nor_flash::NorFlash;
use sequential_storage::cache::{Cache, Uncached};
use sequential_storage::map::{MapConfig, MapStorage, PostcardValue};
use serde::{Deserialize, Serialize};

use crate::apps::AppId;
use crate::calibration::Calibration;
use crate::platform::ENCODERS;

/// Version of the stored format, also the map key it is stored under.
///
/// A change to [`Settings`] that `postcard` cannot read back (a field added,
/// removed or reordered) must bump this: settings stored in an older format
/// are then ignored, and the module falls back to [`Settings::DEFAULT`]
/// rather than misreading them.
pub const FORMAT: u8 = 1;

/// Size of one erasable sector of the settings flash, in bytes: the 4 KiB
/// sector of the Teensy 4.0's NOR flash.
pub const SECTOR_BYTES: u32 = 4_096;

/// Number of sectors the settings rotate through, for wear levelling.
pub const SECTORS: u32 = 4;

/// Size of the flash a backend provides for the settings, in bytes. The
/// store uses addresses `0..FLASH_BYTES` of the [`NorFlash`] it is given.
pub const FLASH_BYTES: u32 = SECTOR_BYTES * SECTORS;

/// Upper bound on one encoded settings record, format byte or map key
/// included, rounded up to any plausible flash word.
pub const ENCODED_MAX: usize = 256;

/// Screen preferences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplaySettings {
    /// Controller contrast (`0x81` command), 0 to 255.
    pub contrast: u8,
    /// Whether the picture is turned by 180°, for a module mounted upside
    /// down.
    pub rotated: bool,
    /// Columns to shift the picture by, relative to the controller's own
    /// offset ("Center Display" in the reference firmware).
    pub column_shift: i8,
}

impl DisplaySettings {
    /// What the display driver sets up on its own: contrast `0xCF`, upright,
    /// no shift.
    pub const DEFAULT: Self = Self {
        contrast: 0xCF,
        rotated: false,
        column_shift: 0,
    };
}

/// Everything the module remembers across power cycles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// Calibration of every CV input and output.
    pub calibration: Calibration,
    /// The applet the module starts in.
    pub startup_app: AppId,
    /// Screen preferences.
    pub display: DisplaySettings,
    /// Whether each encoder's direction is reversed, left then right.
    pub encoder_reversed: [bool; ENCODERS],
}

impl Settings {
    /// A module that has never been set up: factory calibration, diagnostic
    /// applet, upright screen, encoders as wired.
    pub const DEFAULT: Self = Self {
        calibration: Calibration::FACTORY,
        startup_app: AppId::Diagnostic,
        display: DisplaySettings::DEFAULT,
        encoder_reversed: [false; ENCODERS],
    };
}

impl Default for Settings {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl PostcardValue<'_> for Settings {}

/// Encodes settings into `buffer`, format byte first; returns the length
/// used, or `None` if the buffer is too small ([`ENCODED_MAX`] always
/// suffices).
#[must_use]
pub fn encode(settings: &Settings, buffer: &mut [u8]) -> Option<usize> {
    let (format, body) = buffer.split_first_mut()?;
    *format = FORMAT;
    let used = postcard::to_slice(settings, body).ok()?.len();
    Some(used + 1)
}

/// Decodes settings written by [`encode`]; `None` for anything else,
/// including settings in another format.
#[must_use]
pub fn decode(bytes: &[u8]) -> Option<Settings> {
    let (&format, body) = bytes.split_first()?;
    if format != FORMAT {
        return None;
    }
    postcard::from_bytes(body).ok()
}

/// Why the settings could not be loaded or saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    /// The flash itself reported an error.
    Flash,
    /// The stored data is corrupted beyond what the store can repair.
    Corrupted,
    /// The settings flash is full; it should have been reclaimed, so this is
    /// a bug in the sizing.
    Full,
    /// The stored record could not be decoded as [`Settings`].
    Format,
    /// The storage crate reported an internal inconsistency.
    Internal,
}

impl<E> From<sequential_storage::Error<E>> for StoreError {
    fn from(error: sequential_storage::Error<E>) -> Self {
        match error {
            sequential_storage::Error::Storage { .. } => Self::Flash,
            sequential_storage::Error::Corrupted { .. } => Self::Corrupted,
            sequential_storage::Error::FullStorage => Self::Full,
            sequential_storage::Error::SerializationError(_) => Self::Format,
            _ => Self::Internal,
        }
    }
}

/// Where the settings came from when the module started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadOutcome {
    /// Settings were found and decoded.
    Loaded,
    /// Nothing stored yet (or only an older format): defaults in use.
    Empty,
    /// The store failed; defaults in use, and the reason.
    Failed(StoreError),
}

/// No RAM cache: one record, read once at boot and written on request, does
/// not repay the bookkeeping.
type NoCache = Cache<Uncached, Uncached, Uncached, u8>;

/// The settings kept in a [`NorFlash`], at addresses `0..FLASH_BYTES`.
pub struct SettingsStore<F: NorFlash> {
    map: MapStorage<u8, F, NoCache>,
    buffer: [u8; ENCODED_MAX],
}

impl<F: NorFlash> core::fmt::Debug for SettingsStore<F> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SettingsStore").finish_non_exhaustive()
    }
}

impl<F: NorFlash> SettingsStore<F> {
    /// The flash range the store manages.
    const RANGE: Range<u32> = 0..FLASH_BYTES;

    /// Wraps a flash device. Nothing is read until [`Self::load`].
    pub const fn new(flash: F) -> Self {
        Self {
            map: MapStorage::new(flash, MapConfig::new(Self::RANGE), Cache::new_uncached()),
            buffer: [0; ENCODED_MAX],
        }
    }

    /// The most recently saved settings, `None` if there are none in the
    /// current [`FORMAT`].
    ///
    /// # Errors
    ///
    /// Returns an error if the flash fails or holds data the store cannot
    /// make sense of.
    pub fn load(&mut self) -> Result<Option<Settings>, StoreError> {
        let fetched =
            embassy_futures::block_on(self.map.fetch_item::<Settings>(&mut self.buffer, &FORMAT))?;
        Ok(fetched)
    }

    /// Loads the settings, falling back to [`Settings::DEFAULT`] whatever
    /// went wrong: a module must always start, and the outcome says why it
    /// may have started unconfigured.
    pub fn load_or_default(&mut self) -> (Settings, LoadOutcome) {
        match self.load() {
            Ok(Some(settings)) => (settings, LoadOutcome::Loaded),
            Ok(None) => (Settings::DEFAULT, LoadOutcome::Empty),
            Err(error) => (Settings::DEFAULT, LoadOutcome::Failed(error)),
        }
    }

    /// Saves the settings; they replace the previous ones only once fully
    /// written, so a power cut leaves either the old or the new settings.
    ///
    /// # Errors
    ///
    /// Returns an error if the flash fails or the store is inconsistent.
    pub fn save(&mut self, settings: &Settings) -> Result<(), StoreError> {
        embassy_futures::block_on(self.map.store_item(&mut self.buffer, &FORMAT, settings))?;
        Ok(())
    }

    /// The flash device, for a host that mirrors it elsewhere (a file).
    pub const fn flash(&mut self) -> &mut F {
        self.map.flash()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DisplaySettings, ENCODED_MAX, FORMAT, LoadOutcome, Settings, SettingsStore, decode, encode,
    };
    use crate::apps::AppId;
    use crate::calibration::{Calibration, CvInputCalibration, CvOutputCalibration};
    use crate::testing::MemoryFlash;

    /// Settings with every field away from its default, and as wide as the
    /// encoding can make them.
    fn unusual() -> Settings {
        Settings {
            calibration: Calibration {
                inputs: [CvInputCalibration {
                    zero_code: i32::MIN,
                    nanovolts_per_code: i32::MAX,
                }; 4],
                outputs: [CvOutputCalibration {
                    codes: [u16::MAX; 11],
                }; 4],
            },
            startup_app: AppId::Scope,
            display: DisplaySettings {
                contrast: 0x10,
                rotated: true,
                column_shift: -2,
            },
            encoder_reversed: [true, false],
        }
    }

    #[test]
    fn the_widest_settings_fit_the_encoding_bound() {
        let mut buffer = [0; ENCODED_MAX];
        let used = encode(&unusual(), &mut buffer).unwrap();
        // One more byte for the map key, the rest for the flash word rounding.
        assert!(used + 1 + 8 <= ENCODED_MAX, "{used} bytes");
    }

    #[test]
    fn encoding_round_trips() {
        let mut buffer = [0; ENCODED_MAX];
        for settings in [Settings::DEFAULT, unusual()] {
            let used = encode(&settings, &mut buffer).unwrap();
            assert_eq!(decode(&buffer[..used]), Some(settings));
        }
    }

    #[test]
    fn another_format_or_garbage_is_not_decoded() {
        let mut buffer = [0; ENCODED_MAX];
        let used = encode(&unusual(), &mut buffer).unwrap();
        buffer[0] = FORMAT + 1;
        assert_eq!(decode(&buffer[..used]), None);
        assert_eq!(decode(&[]), None);
        assert_eq!(decode(&[FORMAT]), None);
        assert_eq!(decode(&[FORMAT, 0xFF, 0xFF]), None);
    }

    #[test]
    fn a_blank_flash_starts_with_the_defaults() {
        let mut store = SettingsStore::new(MemoryFlash::erased());
        assert_eq!(store.load(), Ok(None));
        assert_eq!(
            store.load_or_default(),
            (Settings::DEFAULT, LoadOutcome::Empty)
        );
    }

    #[test]
    fn saved_settings_survive_a_power_cycle() {
        let mut store = SettingsStore::new(MemoryFlash::erased());
        store.save(&unusual()).unwrap();

        // A fresh store over the same bytes is a module after power-up.
        let image = *store.flash().as_bytes();
        let mut restarted = SettingsStore::new(MemoryFlash::from_image(image));
        assert_eq!(
            restarted.load_or_default(),
            (unusual(), LoadOutcome::Loaded)
        );
    }

    #[test]
    fn the_latest_save_wins_and_many_saves_wrap_around_the_sectors() {
        let mut store = SettingsStore::new(MemoryFlash::erased());
        let mut settings = Settings::DEFAULT;
        // Far more saves than fit in the flash at once: sectors must be
        // reclaimed as they fill.
        for contrast in 0..=255u8 {
            settings.display.contrast = contrast;
            store.save(&settings).unwrap();
        }
        for round in 0..200u8 {
            settings.display.contrast = round;
            store.save(&settings).unwrap();
        }
        assert_eq!(store.load(), Ok(Some(settings)));
        assert!(store.flash().erase_count() > 0, "sectors were reclaimed");
    }

    #[test]
    fn foreign_data_falls_back_to_the_defaults() {
        // Whatever another firmware left in the sectors (here: a pattern that
        // is neither erased flash nor a valid page), the module still starts.
        let mut image = [0u8; super::FLASH_BYTES as usize];
        for (index, byte) in image.iter_mut().enumerate() {
            *byte = u8::try_from(index % 251).unwrap();
        }
        let mut store = SettingsStore::new(MemoryFlash::from_image(image));
        let (settings, _) = store.load_or_default();
        assert_eq!(settings, Settings::DEFAULT);

        // And the first save over it must still succeed and read back.
        store.save(&unusual()).unwrap();
        assert_eq!(store.load(), Ok(Some(unusual())));
    }
}
