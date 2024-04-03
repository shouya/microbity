MEMORY
{
  /* https://infocenter.nordicsemi.com/topic/sds_s113/SDS/s1xx/mem_usage/mem_resource_map_usage.html */
  /* NOTE 1 K = 1 KiBi = 1024 bytes */
  FLASH : ORIGIN = 0x00000000, LENGTH = 512K
  RAM : ORIGIN = 0x20000000, LENGTH = 128K
}
