MEMORY
{
  /* https://infocenter.nordicsemi.com/topic/sds_s113/SDS/s1xx/mem_usage/mem_resource_map_usage.html */
  /* NOTE 1 K = 1 KiBi = 1024 bytes */
  FLASH : ORIGIN = APP_CODE_BASE, LENGTH = 512K - APP_CODE_BASE
  RAM : ORIGIN = APP_RAM_BASE, LENGTH = 128K - APP_RAM_BASE
}
