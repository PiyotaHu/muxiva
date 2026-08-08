#include "muxiva/muxiva.h"

int main(void) {
  muxiva_runtime_v1 runtime = {0, 0};
  return (sizeof(runtime) == 16 && muxiva_abi_version_v1() == MUXIVA_ABI_VERSION_V1) ? 0 : 1;
}
