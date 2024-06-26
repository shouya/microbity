#[cfg(feature = "app_ble_temp")]
pub mod ble_temp;
#[cfg(feature = "app_i2c_display")]
pub mod i2c_display;
#[cfg(feature = "app_midi_player")]
pub mod midi_player;
#[cfg(feature = "app_pcm_player")]
pub mod pcm_player;
#[cfg(feature = "app_playground")]
pub mod playground;
#[cfg(feature = "app_temp")]
pub mod temp;
#[cfg(feature = "app_tone_generator")]
pub mod tone_generator;
#[cfg(feature = "app_volume")]
pub mod volume;
