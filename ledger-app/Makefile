ifeq ($(BOLOS_SDK),)
$(error Environment variable BOLOS_SDK is not set)
endif

APPNAME = "Ledger Squads"
APPVERSION_M = 0
APPVERSION_N = 1
APPVERSION_P = 0
APPVERSION = "$(APPVERSION_M).$(APPVERSION_N).$(APPVERSION_P)"

ICONNAME = glyphs/icon.gif

APP_SOURCE_PATH += src
APP_SOURCE_FILES += src/main.c src/storage.c src/squads_tx.c src/ui.c
APP_SOURCE_FILES += ${BOLOS_SDK}/lib_standard_app/crypto_helpers.c

CURVE_APP_LOAD_PARAMS = ed25519
PATH_APP_LOAD_PARAMS = "44'/501'"

ENABLE_BLUETOOTH = 0
ENABLE_NBGL_FOR_NANO_DEVICES = 1
ENABLE_APP_STORAGE = 1
ENABLE_APP_STORAGE_PROP_SETTINGS = 0
ENABLE_APP_STORAGE_PROP_DATA = 1
APP_STORAGE_SIZE = 4096

CFLAGS += -I${BOLOS_SDK}/lib_standard_app/

include $(BOLOS_SDK)/Makefile.standard_app
