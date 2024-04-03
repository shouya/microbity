/*
Flash the softdevice via
probe-rs download --verify --format hex --chip nRF52833_xxAA ~/Downloads/s113_nrf52_7.3.0/s113_nrf52_7.3.0_softdevice.hex

The size can be found in the "SoftDevice properties" section of the release note inside the downloaded soft device zip file.
*/

MEMORY
{
  /* https://infocenter.nordicsemi.com/topic/sds_s113/SDS/s1xx/mem_usage/mem_resource_map_usage.html */
  /* NOTE 1 K = 1 KiBi = 1024 bytes */
  FLASH : ORIGIN = 0x00000000 + 112K, LENGTH = 512K - 112K
  RAM : ORIGIN = 0x20000000 + 8K, LENGTH = 128K - 8K
}
