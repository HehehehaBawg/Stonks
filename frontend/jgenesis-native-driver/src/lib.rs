pub mod config;
pub mod input;
mod mainloop;

pub use mainloop::{
    create_genesis, create_nes, create_sega_cd, create_smsgg, create_snes, AudioError,
    NativeEmulator, NativeEmulatorResult, NativeGenesisEmulator, NativeNesEmulator,
    NativeSegaCdEmulator, NativeSmsGgEmulator, NativeSnesEmulator, NativeTickEffect,
    SaveWriteError,
};
