#pragma once

#include "app.h"

#define SQUADS_PROGRAM_ID                                                                            \
    {                                                                                                \
        0x06, 0x81, 0xC4, 0xCE, 0x47, 0xE2, 0x23, 0x68, 0xB8, 0xB1, 0x55, 0x5E, 0xC8, 0x87, 0xAF,  \
            0x09, 0x2E, 0xFC, 0x7E, 0xFB, 0xB6, 0x6C, 0xA3, 0xF5, 0x2F, 0xBF, 0x68, 0xD4, 0xAC,    \
            0x9C, 0xB7, 0xA8                                                                         \
    }

#define SYSTEM_PROGRAM_ID                                                                            \
    {                                                                                                \
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  \
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,    \
            0x00, 0x00, 0x00                                                                         \
    }

#define BPF_LOADER_UPGRADEABLE_PROGRAM_ID                                                            \
    {                                                                                                \
        0x02, 0xA8, 0xF6, 0x91, 0x4E, 0x88, 0xA1, 0xB0, 0xE2, 0x10, 0x15, 0x3E, 0xF7, 0x63, 0xAE,  \
            0x2B, 0x00, 0xC2, 0xB9, 0x3D, 0x16, 0xC1, 0x24, 0xD2, 0xC0, 0x53, 0x7A, 0x10, 0x04,    \
            0x80, 0x00, 0x00                                                                         \
    }

#define SYSVAR_RENT_ID                                                                               \
    {                                                                                                \
        0x06, 0xA7, 0xD5, 0x17, 0x19, 0x2C, 0x5C, 0x51, 0x21, 0x8C, 0xC9, 0x4C, 0x3D, 0x4A, 0xF1,  \
            0x7F, 0x58, 0xDA, 0xEE, 0x08, 0x9B, 0xA1, 0xFD, 0x44, 0xE3, 0xDB, 0xD9, 0x8A, 0x00,    \
            0x00, 0x00, 0x00                                                                         \
    }

#define SYSVAR_CLOCK_ID                                                                              \
    {                                                                                                \
        0x06, 0xA7, 0xD5, 0x17, 0x18, 0xC7, 0x74, 0xC9, 0x28, 0x56, 0x63, 0x98, 0x69, 0x1D, 0x5E,  \
            0xB6, 0x8B, 0x5E, 0xB8, 0xA3, 0x9B, 0x4B, 0x6D, 0x5C, 0x73, 0x55, 0x5B, 0x21, 0x00,    \
            0x00, 0x00, 0x00                                                                         \
    }

bool parse_derivation_path(const uint8_t *data,
                           size_t data_length,
                           uint32_t derivation_path[MAX_DERIVATION_PATH_LENGTH],
                           uint8_t *path_length,
                           size_t *consumed);

cx_err_t derive_public_key(uint8_t out[PUBKEY_LENGTH],
                           const uint32_t *derivation_path,
                           size_t path_length);

cx_err_t sign_message_with_path(const uint32_t *derivation_path,
                                size_t path_length,
                                const uint8_t *message,
                                size_t message_length,
                                uint8_t signature[SIGNATURE_LENGTH]);

bool derive_proposal_pda(const uint8_t multisig[PUBKEY_LENGTH],
                         uint64_t transaction_index,
                         uint8_t proposal[PUBKEY_LENGTH]);
bool derive_transaction_pda(const uint8_t multisig[PUBKEY_LENGTH],
                            uint64_t transaction_index,
                            uint8_t transaction[PUBKEY_LENGTH]);
bool derive_vault_pda(const uint8_t multisig[PUBKEY_LENGTH],
                      uint8_t vault_index,
                      uint8_t vault[PUBKEY_LENGTH]);
bool derive_program_data_pda(const uint8_t program[PUBKEY_LENGTH], uint8_t program_data[PUBKEY_LENGTH]);

bool build_upgrade_intent_hash(const uint8_t multisig[PUBKEY_LENGTH],
                               uint8_t vault_index,
                               const uint8_t program[PUBKEY_LENGTH],
                               const uint8_t buffer[PUBKEY_LENGTH],
                               const uint8_t spill[PUBKEY_LENGTH],
                               uint8_t out_intent_hash[MESSAGE_HASH_LENGTH]);

bool build_proposal_vote_message(const uint8_t member[PUBKEY_LENGTH],
                                 const uint8_t multisig[PUBKEY_LENGTH],
                                 const uint8_t proposal[PUBKEY_LENGTH],
                                 uint8_t vote,
                                 const uint8_t recent_blockhash[BLOCKHASH_LENGTH],
                                 uint8_t out_message[MAX_MESSAGE_LENGTH],
                                 size_t *out_message_length);
bool build_upgrade_create_transaction_message(const uint8_t member[PUBKEY_LENGTH],
                                              const uint8_t multisig[PUBKEY_LENGTH],
                                              const uint8_t transaction[PUBKEY_LENGTH],
                                              uint8_t vault_index,
                                              const uint8_t program[PUBKEY_LENGTH],
                                              const uint8_t buffer[PUBKEY_LENGTH],
                                              const uint8_t spill[PUBKEY_LENGTH],
                                              const uint8_t recent_blockhash[BLOCKHASH_LENGTH],
                                              uint8_t out_message[MAX_MESSAGE_LENGTH],
                                              size_t *out_message_length);
bool build_proposal_create_message(const uint8_t member[PUBKEY_LENGTH],
                                   const uint8_t multisig[PUBKEY_LENGTH],
                                   const uint8_t proposal[PUBKEY_LENGTH],
                                   uint64_t transaction_index,
                                   const uint8_t recent_blockhash[BLOCKHASH_LENGTH],
                                   uint8_t out_message[MAX_MESSAGE_LENGTH],
                                   size_t *out_message_length);
bool build_upgrade_execute_message(const uint8_t member[PUBKEY_LENGTH],
                                   const uint8_t multisig[PUBKEY_LENGTH],
                                   const uint8_t proposal[PUBKEY_LENGTH],
                                   const uint8_t transaction[PUBKEY_LENGTH],
                                   uint8_t vault_index,
                                   const uint8_t program[PUBKEY_LENGTH],
                                   const uint8_t buffer[PUBKEY_LENGTH],
                                   const uint8_t spill[PUBKEY_LENGTH],
                                   const uint8_t recent_blockhash[BLOCKHASH_LENGTH],
                                   uint8_t out_message[MAX_MESSAGE_LENGTH],
                                   size_t *out_message_length);

void sha256_bytes(const uint8_t *data, size_t length, uint8_t out[MESSAGE_HASH_LENGTH]);
void app_format_short_pubkey(const uint8_t pubkey[PUBKEY_LENGTH], char out[18]);
void app_format_path(const uint32_t *path, uint8_t path_length, char *out, size_t out_length);
void app_format_u64(uint64_t value, char *out, size_t out_length);
void app_format_hex(const uint8_t *bytes, size_t length, char *out, size_t out_length);
