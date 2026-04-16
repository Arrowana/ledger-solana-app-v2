#pragma once

#include "app.h"

#if defined(TARGET_NANOX) || defined(TARGET_NANOS2)
#define ICON_HOME      C_icon
#define ICON_SIGN_MENU C_icon_certificate
#define ICON_WARNING   C_icon_warning
#define ICON_REVIEW    C_icon_certificate
#elif defined(TARGET_STAX) || defined(TARGET_FLEX)
#define ICON_HOME      C_icon
#define ICON_SIGN_MENU C_icon
#define ICON_WARNING   C_icon_warning
#define ICON_REVIEW    C_icon_certificate
#elif defined(TARGET_APEX_P)
#define ICON_HOME      C_icon
#define ICON_SIGN_MENU C_icon
#define ICON_WARNING   C_icon_warning
#define ICON_REVIEW    C_icon_certificate
#endif

void ui_settings(void);
void ui_multisig_modal(bool is_success);
void ui_transaction_modal(bool is_success);
