#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

#include "os.h"
#include "cx.h"
#include "os_io_seproxyhal.h"
#include "ux.h"
#include "io.h"

#define APP_CLA 0xE0

#define INS_GET_VERSION 0x00
#define INS_SAVE_MULTISIG 0x10
#define INS_LIST_MULTISIG_SLOT 0x11
#define INS_PROPOSAL_VOTE 0x12
#define INS_RESET_MULTISIGS 0x13
#define INS_PROPOSAL_CREATE_UPGRADE 0x14
#define INS_PROPOSAL_EXECUTE_UPGRADE 0x15

#define P1_CONFIRM 0x00
#define P1_NON_CONFIRM 0x01

#define PROPOSAL_VOTE_APPROVE 0x00
#define PROPOSAL_VOTE_REJECT 0x01

#define APDU_OFFSET_CLA 0
#define APDU_OFFSET_INS 1
#define APDU_OFFSET_P1 2
#define APDU_OFFSET_P2 3
#define APDU_OFFSET_LC 4
#define APDU_OFFSET_CDATA 5

#define SW_OK 0x9000
#define SW_USER_REFUSED 0x6985
#define SW_CONDITIONS_NOT_SATISFIED 0x6986
#define SW_INVALID_DATA 0x6A80
#define SW_NOT_FOUND 0x6A88
#define SW_INS_NOT_SUPPORTED 0x6D00
#define SW_CLA_NOT_SUPPORTED 0x6E00
#define SW_UNKNOWN 0x6F00

#define PUBKEY_LENGTH 32
#define SIGNATURE_LENGTH 64
#define BLOCKHASH_LENGTH 32
#define MAX_DERIVATION_PATH_LENGTH 5
#define MAX_SAVED_MULTISIGS 8
#define MAX_REVIEWED_UPGRADE_INTENTS 8
#define MESSAGE_HASH_LENGTH 32
#define MAX_MESSAGE_LENGTH 512
#define MAX_RESPONSE_LENGTH 256

typedef struct {
    uint8_t occupied;
    uint8_t multisig[PUBKEY_LENGTH];
    uint8_t member[PUBKEY_LENGTH];
    uint8_t path_length;
    uint32_t derivation_path[MAX_DERIVATION_PATH_LENGTH];
} saved_multisig_entry_t;

typedef struct {
    uint8_t occupied;
    uint8_t multisig[PUBKEY_LENGTH];
    uint64_t transaction_index;
    uint8_t vault_index;
    uint8_t program[PUBKEY_LENGTH];
    uint8_t buffer[PUBKEY_LENGTH];
    uint8_t spill[PUBKEY_LENGTH];
    uint8_t intent_hash[MESSAGE_HASH_LENGTH];
} reviewed_upgrade_intent_t;

typedef struct {
    uint8_t initialized;
    saved_multisig_entry_t entries[MAX_SAVED_MULTISIGS];
    reviewed_upgrade_intent_t reviewed_upgrades[MAX_REVIEWED_UPGRADE_INTENTS];
} internal_storage_t;

extern ux_state_t G_ux;
extern bolos_ux_params_t G_ux_params;
extern unsigned char G_io_seproxyhal_spi_buffer[IO_SEPROXYHAL_BUFFER_SIZE_B];

void ui_idle(void);
void ui_show_save_review(const char *multisig, const char *member, const char *path);
void ui_show_vote_review(const char *multisig,
                         const char *action,
                         const char *member,
                         const char *transaction_index,
                         const char *message_hash);
void ui_show_create_upgrade_review(const char *multisig,
                                   const char *transaction_index,
                                   const char *vault_index,
                                   const char *program,
                                   const char *buffer,
                                   const char *spill,
                                   const char *intent_hash,
                                   const char *transaction_hash,
                                   const char *proposal_hash);
void ui_show_execute_upgrade_review(const char *multisig,
                                    const char *transaction_index,
                                    const char *vault_index,
                                    const char *program,
                                    const char *buffer,
                                    const char *spill,
                                    const char *intent_hash,
                                    const char *message_hash);
void ui_show_reset_review(void);

void app_approve_pending(void);
void app_reject_pending(void);
void app_exit(void);

int io_send_response(const uint8_t *buffer, size_t length, uint16_t status_word);
int io_send_status(uint16_t status_word);
