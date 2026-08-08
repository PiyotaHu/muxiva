#include "muxiva/rtc_adapter_v1.h"
#include <stddef.h>
_Static_assert(offsetof(muxiva_rtc_adapter_config_v1, abi_version) == 0, "ABI prefix drift");
_Static_assert(offsetof(muxiva_rtc_callbacks_v1, struct_size) == sizeof(uint32_t), "ABI size drift");
int main(void) { return MUXIVA_RTC_CREATED == 1 ? 0 : 1; }
