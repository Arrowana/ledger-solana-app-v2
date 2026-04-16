#include "squads_tx.h"

#include <stdio.h>

#include "lib_standard_app/crypto_helpers.h"

typedef struct {
    const uint8_t *data;
    size_t length;
} seed_buffer_t;

static const uint8_t G_squads_program_id[PUBKEY_LENGTH] = SQUADS_PROGRAM_ID;
static const uint8_t G_system_program_id[PUBKEY_LENGTH] = SYSTEM_PROGRAM_ID;
static const uint8_t G_bpf_loader_upgradeable_program_id[PUBKEY_LENGTH] =
    BPF_LOADER_UPGRADEABLE_PROGRAM_ID;
static const uint8_t G_sysvar_rent_id[PUBKEY_LENGTH] = SYSVAR_RENT_ID;
static const uint8_t G_sysvar_clock_id[PUBKEY_LENGTH] = SYSVAR_CLOCK_ID;
static const uint8_t G_proposal_approve_discriminator[8] = {144, 37, 164, 136, 188, 216, 42, 248};
static const uint8_t G_proposal_reject_discriminator[8] = {243, 62, 134, 156, 230, 106, 246, 135};
static const uint8_t G_vault_transaction_create_discriminator[8] = {
    48, 250, 78, 168, 208, 226, 218, 211};
static const uint8_t G_proposal_create_discriminator[8] = {220, 60, 73, 224, 30, 108, 79, 159};
static const uint8_t G_vault_transaction_execute_discriminator[8] = {
    194, 8, 161, 87, 153, 164, 25, 171};
static const uint8_t G_upgradeable_loader_upgrade_instruction_data[4] = {3, 0, 0, 0};
static const char *G_pda_marker = "ProgramDerivedAddress";
static const uint8_t G_seed_prefix[] = {'m', 'u', 'l', 't', 'i', 's', 'i', 'g'};
static const uint8_t G_seed_transaction[] = {'t', 'r', 'a', 'n', 's', 'a', 'c', 't', 'i', 'o', 'n'};
static const uint8_t G_seed_proposal[] = {'p', 'r', 'o', 'p', 'o', 's', 'a', 'l'};
static const uint8_t G_seed_vault[] = {'v', 'a', 'u', 'l', 't'};

static bool write_bytes(uint8_t *buffer,
                        size_t capacity,
                        size_t *offset,
                        const uint8_t *bytes,
                        size_t length) {
    if (*offset + length > capacity) {
        return false;
    }
    memcpy(buffer + *offset, bytes, length);
    *offset += length;
    return true;
}

static bool write_u8(uint8_t *buffer, size_t capacity, size_t *offset, uint8_t value) {
    return write_bytes(buffer, capacity, offset, &value, 1);
}

static bool write_u16_le(uint8_t *buffer, size_t capacity, size_t *offset, uint16_t value) {
    uint8_t encoded[2] = {(uint8_t) (value & 0xFF), (uint8_t) ((value >> 8) & 0xFF)};
    return write_bytes(buffer, capacity, offset, encoded, sizeof(encoded));
}

static bool write_u32_le(uint8_t *buffer, size_t capacity, size_t *offset, uint32_t value) {
    uint8_t encoded[4] = {
        (uint8_t) (value & 0xFF),
        (uint8_t) ((value >> 8) & 0xFF),
        (uint8_t) ((value >> 16) & 0xFF),
        (uint8_t) ((value >> 24) & 0xFF),
    };
    return write_bytes(buffer, capacity, offset, encoded, sizeof(encoded));
}

static bool write_u64_le(uint8_t *buffer, size_t capacity, size_t *offset, uint64_t value) {
    uint8_t encoded[8];
    for (size_t index = 0; index < sizeof(encoded); index++) {
        encoded[index] = (uint8_t) ((value >> (index * 8)) & 0xFF);
    }
    return write_bytes(buffer, capacity, offset, encoded, sizeof(encoded));
}

static bool encode_shortvec(size_t value, uint8_t *out, size_t *written) {
    size_t cursor = 0;

    do {
        uint8_t byte = (uint8_t) (value & 0x7F);
        value >>= 7;
        if (value != 0) {
            byte |= 0x80;
        }
        out[cursor++] = byte;
    } while (value != 0);

    *written = cursor;
    return true;
}

static bool write_shortvec(uint8_t *buffer, size_t capacity, size_t *offset, size_t value) {
    uint8_t encoded[10];
    size_t written = 0;
    if (!encode_shortvec(value, encoded, &written)) {
        return false;
    }
    return write_bytes(buffer, capacity, offset, encoded, written);
}

static bool is_on_curve(const uint8_t compressed_point[PUBKEY_LENGTH]) {
    cx_ecpoint_t point;
    cx_err_t err;
    bool on_curve = false;
    uint8_t local[PUBKEY_LENGTH];

    memset(&point, 0, sizeof(point));
    memcpy(local, compressed_point, PUBKEY_LENGTH);

    err = cx_ecpoint_alloc(&point, CX_CURVE_Ed25519);
    if (err != CX_OK) {
        return false;
    }

    int sign = cx_decode_coord(local, PUBKEY_LENGTH);
    err = cx_ecpoint_decompress(&point, local, PUBKEY_LENGTH, sign);
    if (err != CX_OK) {
        return false;
    }

    err = cx_ecpoint_is_on_curve(&point, &on_curve);
    if (err != CX_OK) {
        return false;
    }

    return on_curve;
}

static bool derive_pda(const seed_buffer_t *seeds,
                       size_t seed_count,
                       const uint8_t program_id[PUBKEY_LENGTH],
                       uint8_t out[PUBKEY_LENGTH]) {
    for (int bump = 255; bump >= 0; bump--) {
        cx_sha256_t hash;
        cx_sha256_init(&hash);

        for (size_t index = 0; index < seed_count; index++) {
            if (cx_hash_no_throw((cx_hash_t *) &hash,
                                 0,
                                 seeds[index].data,
                                 seeds[index].length,
                                 NULL,
                                 0) != CX_OK) {
                return false;
            }
        }

        uint8_t bump_byte = (uint8_t) bump;
        if (cx_hash_no_throw((cx_hash_t *) &hash, 0, &bump_byte, 1, NULL, 0) != CX_OK) {
            return false;
        }
        if (cx_hash_no_throw((cx_hash_t *) &hash, 0, program_id, PUBKEY_LENGTH, NULL, 0) !=
            CX_OK) {
            return false;
        }
        if (cx_hash_no_throw((cx_hash_t *) &hash,
                             CX_LAST,
                             (const uint8_t *) G_pda_marker,
                             strlen(G_pda_marker),
                             out,
                             PUBKEY_LENGTH) != CX_OK) {
            return false;
        }

        if (!is_on_curve(out)) {
            return true;
        }
    }

    return false;
}

static bool build_upgrade_wrapped_message(const uint8_t vault[PUBKEY_LENGTH],
                                          const uint8_t program[PUBKEY_LENGTH],
                                          const uint8_t buffer[PUBKEY_LENGTH],
                                          const uint8_t spill[PUBKEY_LENGTH],
                                          uint8_t out_message[MAX_MESSAGE_LENGTH],
                                          size_t *out_message_length) {
    uint8_t program_data[PUBKEY_LENGTH];
    size_t offset = 0;
    const uint8_t account_indexes[] = {1, 2, 3, 4, 6, 7, 0};

    if (!derive_program_data_pda(program, program_data)) {
        return false;
    }

    if (!write_u8(out_message, MAX_MESSAGE_LENGTH, &offset, 1) ||
        !write_u8(out_message, MAX_MESSAGE_LENGTH, &offset, 1) ||
        !write_u8(out_message, MAX_MESSAGE_LENGTH, &offset, 4)) {
        return false;
    }

    if (!write_u8(out_message, MAX_MESSAGE_LENGTH, &offset, 8) ||
        !write_bytes(out_message, MAX_MESSAGE_LENGTH, &offset, vault, PUBKEY_LENGTH) ||
        !write_bytes(out_message, MAX_MESSAGE_LENGTH, &offset, program_data, PUBKEY_LENGTH) ||
        !write_bytes(out_message, MAX_MESSAGE_LENGTH, &offset, program, PUBKEY_LENGTH) ||
        !write_bytes(out_message, MAX_MESSAGE_LENGTH, &offset, buffer, PUBKEY_LENGTH) ||
        !write_bytes(out_message, MAX_MESSAGE_LENGTH, &offset, spill, PUBKEY_LENGTH) ||
        !write_bytes(out_message,
                     MAX_MESSAGE_LENGTH,
                     &offset,
                     G_bpf_loader_upgradeable_program_id,
                     PUBKEY_LENGTH) ||
        !write_bytes(out_message, MAX_MESSAGE_LENGTH, &offset, G_sysvar_rent_id, PUBKEY_LENGTH) ||
        !write_bytes(out_message, MAX_MESSAGE_LENGTH, &offset, G_sysvar_clock_id, PUBKEY_LENGTH)) {
        return false;
    }

    if (!write_u8(out_message, MAX_MESSAGE_LENGTH, &offset, 1) ||
        !write_u8(out_message, MAX_MESSAGE_LENGTH, &offset, 5) ||
        !write_u8(out_message, MAX_MESSAGE_LENGTH, &offset, sizeof(account_indexes)) ||
        !write_bytes(out_message,
                     MAX_MESSAGE_LENGTH,
                     &offset,
                     account_indexes,
                     sizeof(account_indexes)) ||
        !write_u16_le(out_message,
                      MAX_MESSAGE_LENGTH,
                      &offset,
                      sizeof(G_upgradeable_loader_upgrade_instruction_data)) ||
        !write_bytes(out_message,
                     MAX_MESSAGE_LENGTH,
                     &offset,
                     G_upgradeable_loader_upgrade_instruction_data,
                     sizeof(G_upgradeable_loader_upgrade_instruction_data)) ||
        !write_u8(out_message, MAX_MESSAGE_LENGTH, &offset, 0)) {
        return false;
    }

    *out_message_length = offset;
    return true;
}

static bool append_legacy_message(uint8_t out_message[MAX_MESSAGE_LENGTH],
                                  size_t *out_message_length,
                                  uint8_t required_signatures,
                                  uint8_t readonly_signed_accounts,
                                  uint8_t readonly_unsigned_accounts,
                                  const uint8_t *const *accounts,
                                  size_t account_count,
                                  const uint8_t recent_blockhash[BLOCKHASH_LENGTH],
                                  uint8_t program_id_index,
                                  const uint8_t *account_indexes,
                                  size_t account_index_count,
                                  const uint8_t *instruction_data,
                                  size_t instruction_data_length) {
    size_t offset = 0;

    if (!write_u8(out_message, MAX_MESSAGE_LENGTH, &offset, required_signatures) ||
        !write_u8(out_message, MAX_MESSAGE_LENGTH, &offset, readonly_signed_accounts) ||
        !write_u8(out_message, MAX_MESSAGE_LENGTH, &offset, readonly_unsigned_accounts) ||
        !write_shortvec(out_message, MAX_MESSAGE_LENGTH, &offset, account_count)) {
        return false;
    }

    for (size_t index = 0; index < account_count; index++) {
        if (!write_bytes(out_message,
                         MAX_MESSAGE_LENGTH,
                         &offset,
                         accounts[index],
                         PUBKEY_LENGTH)) {
            return false;
        }
    }

    if (!write_bytes(out_message,
                     MAX_MESSAGE_LENGTH,
                     &offset,
                     recent_blockhash,
                     BLOCKHASH_LENGTH) ||
        !write_u8(out_message, MAX_MESSAGE_LENGTH, &offset, 1) ||
        !write_u8(out_message, MAX_MESSAGE_LENGTH, &offset, program_id_index) ||
        !write_shortvec(out_message, MAX_MESSAGE_LENGTH, &offset, account_index_count) ||
        !write_bytes(out_message,
                     MAX_MESSAGE_LENGTH,
                     &offset,
                     account_indexes,
                     account_index_count) ||
        !write_shortvec(out_message,
                        MAX_MESSAGE_LENGTH,
                        &offset,
                        instruction_data_length) ||
        !write_bytes(out_message,
                     MAX_MESSAGE_LENGTH,
                     &offset,
                     instruction_data,
                     instruction_data_length)) {
        return false;
    }

    *out_message_length = offset;
    return true;
}

bool parse_derivation_path(const uint8_t *data,
                           size_t data_length,
                           uint32_t derivation_path[MAX_DERIVATION_PATH_LENGTH],
                           uint8_t *path_length,
                           size_t *consumed) {
    if (data_length < 1) {
        return false;
    }

    uint8_t count = data[0];
    if (count == 0 || count > MAX_DERIVATION_PATH_LENGTH) {
        return false;
    }
    if (data_length < 1 + (size_t) count * 4) {
        return false;
    }

    for (uint8_t index = 0; index < count; index++) {
        derivation_path[index] = ((uint32_t) data[1 + index * 4] << 24) |
                                 ((uint32_t) data[2 + index * 4] << 16) |
                                 ((uint32_t) data[3 + index * 4] << 8) |
                                 ((uint32_t) data[4 + index * 4]);
    }

    *path_length = count;
    *consumed = 1 + (size_t) count * 4;
    return true;
}

cx_err_t derive_public_key(uint8_t out[PUBKEY_LENGTH],
                           const uint32_t *derivation_path,
                           size_t path_length) {
    uint8_t raw_public_key[65];
    cx_err_t err = bip32_derive_with_seed_get_pubkey_256(HDW_ED25519_SLIP10,
                                                         CX_CURVE_Ed25519,
                                                         derivation_path,
                                                         path_length,
                                                         raw_public_key,
                                                         NULL,
                                                         CX_SHA512,
                                                         NULL,
                                                         0);
    if (err != CX_OK) {
        return err;
    }

    for (size_t index = 0; index < PUBKEY_LENGTH; index++) {
        out[index] = raw_public_key[PUBKEY_LENGTH + 32 - index];
    }
    if ((raw_public_key[PUBKEY_LENGTH] & 1) != 0) {
        out[PUBKEY_LENGTH - 1] |= 0x80;
    }

    return CX_OK;
}

cx_err_t sign_message_with_path(const uint32_t *derivation_path,
                                size_t path_length,
                                const uint8_t *message,
                                size_t message_length,
                                uint8_t signature[SIGNATURE_LENGTH]) {
    size_t signature_length = SIGNATURE_LENGTH;
    return bip32_derive_with_seed_eddsa_sign_hash_256(HDW_ED25519_SLIP10,
                                                      CX_CURVE_Ed25519,
                                                      derivation_path,
                                                      path_length,
                                                      CX_SHA512,
                                                      message,
                                                      message_length,
                                                      signature,
                                                      &signature_length,
                                                      NULL,
                                                      0);
}

void sha256_bytes(const uint8_t *data, size_t length, uint8_t out[MESSAGE_HASH_LENGTH]) {
    cx_sha256_t hash;
    cx_sha256_init(&hash);
    if (cx_hash_no_throw((cx_hash_t *) &hash, CX_LAST, data, length, out, MESSAGE_HASH_LENGTH) !=
        CX_OK) {
        memset(out, 0, MESSAGE_HASH_LENGTH);
    }
}

bool derive_proposal_pda(const uint8_t multisig[PUBKEY_LENGTH],
                         uint64_t transaction_index,
                         uint8_t proposal[PUBKEY_LENGTH]) {
    uint8_t tx_index_le[8];
    seed_buffer_t seeds[5] = {
        {.data = G_seed_prefix, .length = sizeof(G_seed_prefix)},
        {.data = multisig, .length = PUBKEY_LENGTH},
        {.data = G_seed_transaction, .length = sizeof(G_seed_transaction)},
        {.data = tx_index_le, .length = sizeof(tx_index_le)},
        {.data = G_seed_proposal, .length = sizeof(G_seed_proposal)},
    };

    for (size_t index = 0; index < sizeof(tx_index_le); index++) {
        tx_index_le[index] = (uint8_t) ((transaction_index >> (index * 8)) & 0xFF);
    }

    return derive_pda(seeds, sizeof(seeds) / sizeof(seeds[0]), G_squads_program_id, proposal);
}

bool derive_transaction_pda(const uint8_t multisig[PUBKEY_LENGTH],
                            uint64_t transaction_index,
                            uint8_t transaction[PUBKEY_LENGTH]) {
    uint8_t tx_index_le[8];
    seed_buffer_t seeds[4] = {
        {.data = G_seed_prefix, .length = sizeof(G_seed_prefix)},
        {.data = multisig, .length = PUBKEY_LENGTH},
        {.data = G_seed_transaction, .length = sizeof(G_seed_transaction)},
        {.data = tx_index_le, .length = sizeof(tx_index_le)},
    };

    for (size_t index = 0; index < sizeof(tx_index_le); index++) {
        tx_index_le[index] = (uint8_t) ((transaction_index >> (index * 8)) & 0xFF);
    }

    return derive_pda(seeds, sizeof(seeds) / sizeof(seeds[0]), G_squads_program_id, transaction);
}

bool derive_vault_pda(const uint8_t multisig[PUBKEY_LENGTH],
                      uint8_t vault_index,
                      uint8_t vault[PUBKEY_LENGTH]) {
    seed_buffer_t seeds[4] = {
        {.data = G_seed_prefix, .length = sizeof(G_seed_prefix)},
        {.data = multisig, .length = PUBKEY_LENGTH},
        {.data = G_seed_vault, .length = sizeof(G_seed_vault)},
        {.data = &vault_index, .length = 1},
    };

    return derive_pda(seeds, sizeof(seeds) / sizeof(seeds[0]), G_squads_program_id, vault);
}

bool derive_program_data_pda(const uint8_t program[PUBKEY_LENGTH], uint8_t program_data[PUBKEY_LENGTH]) {
    seed_buffer_t seeds[1] = {{.data = program, .length = PUBKEY_LENGTH}};
    return derive_pda(seeds,
                      sizeof(seeds) / sizeof(seeds[0]),
                      G_bpf_loader_upgradeable_program_id,
                      program_data);
}

bool build_upgrade_intent_hash(const uint8_t multisig[PUBKEY_LENGTH],
                               uint8_t vault_index,
                               const uint8_t program[PUBKEY_LENGTH],
                               const uint8_t buffer[PUBKEY_LENGTH],
                               const uint8_t spill[PUBKEY_LENGTH],
                               uint8_t out_intent_hash[MESSAGE_HASH_LENGTH]) {
    uint8_t vault[PUBKEY_LENGTH];
    uint8_t wrapped_message[MAX_MESSAGE_LENGTH];
    size_t wrapped_message_length = 0;

    if (!derive_vault_pda(multisig, vault_index, vault) ||
        !build_upgrade_wrapped_message(vault,
                                       program,
                                       buffer,
                                       spill,
                                       wrapped_message,
                                       &wrapped_message_length)) {
        return false;
    }

    sha256_bytes(wrapped_message, wrapped_message_length, out_intent_hash);
    return true;
}

bool build_proposal_vote_message(const uint8_t member[PUBKEY_LENGTH],
                                 const uint8_t multisig[PUBKEY_LENGTH],
                                 const uint8_t proposal[PUBKEY_LENGTH],
                                 uint8_t vote,
                                 const uint8_t recent_blockhash[BLOCKHASH_LENGTH],
                                 uint8_t out_message[MAX_MESSAGE_LENGTH],
                                 size_t *out_message_length) {
    const uint8_t *accounts[] = {member, proposal, G_squads_program_id, multisig};
    const uint8_t account_indexes[] = {3, 0, 1};
    uint8_t instruction_data[9];

    if (vote == PROPOSAL_VOTE_APPROVE) {
        memcpy(instruction_data,
               G_proposal_approve_discriminator,
               sizeof(G_proposal_approve_discriminator));
    } else if (vote == PROPOSAL_VOTE_REJECT) {
        memcpy(instruction_data,
               G_proposal_reject_discriminator,
               sizeof(G_proposal_reject_discriminator));
    } else {
        return false;
    }
    instruction_data[8] = 0;

    return append_legacy_message(out_message,
                                 out_message_length,
                                 1,
                                 0,
                                 2,
                                 accounts,
                                 sizeof(accounts) / sizeof(accounts[0]),
                                 recent_blockhash,
                                 2,
                                 account_indexes,
                                 sizeof(account_indexes),
                                 instruction_data,
                                 sizeof(instruction_data));
}

bool build_upgrade_create_transaction_message(const uint8_t member[PUBKEY_LENGTH],
                                              const uint8_t multisig[PUBKEY_LENGTH],
                                              const uint8_t transaction[PUBKEY_LENGTH],
                                              uint8_t vault_index,
                                              const uint8_t program[PUBKEY_LENGTH],
                                              const uint8_t buffer[PUBKEY_LENGTH],
                                              const uint8_t spill[PUBKEY_LENGTH],
                                              const uint8_t recent_blockhash[BLOCKHASH_LENGTH],
                                              uint8_t out_message[MAX_MESSAGE_LENGTH],
                                              size_t *out_message_length) {
    const uint8_t *accounts[] = {
        member, multisig, transaction, G_squads_program_id, G_system_program_id};
    const uint8_t account_indexes[] = {1, 2, 0, 0, 4};
    uint8_t vault[PUBKEY_LENGTH];
    uint8_t wrapped_message[MAX_MESSAGE_LENGTH];
    uint8_t instruction_data[MAX_MESSAGE_LENGTH];
    size_t wrapped_message_length = 0;
    size_t instruction_data_length = 0;

    if (!derive_vault_pda(multisig, vault_index, vault) ||
        !build_upgrade_wrapped_message(vault,
                                       program,
                                       buffer,
                                       spill,
                                       wrapped_message,
                                       &wrapped_message_length)) {
        return false;
    }

    if (!write_bytes(instruction_data,
                     sizeof(instruction_data),
                     &instruction_data_length,
                     G_vault_transaction_create_discriminator,
                     sizeof(G_vault_transaction_create_discriminator)) ||
        !write_u8(instruction_data, sizeof(instruction_data), &instruction_data_length, vault_index) ||
        !write_u8(instruction_data, sizeof(instruction_data), &instruction_data_length, 0) ||
        !write_u32_le(instruction_data,
                      sizeof(instruction_data),
                      &instruction_data_length,
                      (uint32_t) wrapped_message_length) ||
        !write_bytes(instruction_data,
                     sizeof(instruction_data),
                     &instruction_data_length,
                     wrapped_message,
                     wrapped_message_length) ||
        !write_u8(instruction_data, sizeof(instruction_data), &instruction_data_length, 0)) {
        return false;
    }

    return append_legacy_message(out_message,
                                 out_message_length,
                                 1,
                                 0,
                                 2,
                                 accounts,
                                 sizeof(accounts) / sizeof(accounts[0]),
                                 recent_blockhash,
                                 3,
                                 account_indexes,
                                 sizeof(account_indexes),
                                 instruction_data,
                                 instruction_data_length);
}

bool build_proposal_create_message(const uint8_t member[PUBKEY_LENGTH],
                                   const uint8_t multisig[PUBKEY_LENGTH],
                                   const uint8_t proposal[PUBKEY_LENGTH],
                                   uint64_t transaction_index,
                                   const uint8_t recent_blockhash[BLOCKHASH_LENGTH],
                                   uint8_t out_message[MAX_MESSAGE_LENGTH],
                                   size_t *out_message_length) {
    const uint8_t *accounts[] = {member, proposal, G_squads_program_id, multisig, G_system_program_id};
    const uint8_t account_indexes[] = {3, 1, 0, 0, 4};
    uint8_t instruction_data[17];
    size_t instruction_data_length = 0;

    if (!write_bytes(instruction_data,
                     sizeof(instruction_data),
                     &instruction_data_length,
                     G_proposal_create_discriminator,
                     sizeof(G_proposal_create_discriminator)) ||
        !write_u64_le(instruction_data,
                      sizeof(instruction_data),
                      &instruction_data_length,
                      transaction_index) ||
        !write_u8(instruction_data, sizeof(instruction_data), &instruction_data_length, 0)) {
        return false;
    }

    return append_legacy_message(out_message,
                                 out_message_length,
                                 1,
                                 0,
                                 3,
                                 accounts,
                                 sizeof(accounts) / sizeof(accounts[0]),
                                 recent_blockhash,
                                 2,
                                 account_indexes,
                                 sizeof(account_indexes),
                                 instruction_data,
                                 instruction_data_length);
}

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
                                   size_t *out_message_length) {
    uint8_t vault[PUBKEY_LENGTH];
    uint8_t program_data[PUBKEY_LENGTH];
    const uint8_t *accounts[] = {
        member,
        proposal,
        vault,
        program_data,
        program,
        buffer,
        spill,
        G_squads_program_id,
        multisig,
        transaction,
        G_bpf_loader_upgradeable_program_id,
        G_sysvar_rent_id,
        G_sysvar_clock_id,
    };
    const uint8_t account_indexes[] = {8, 1, 9, 0, 2, 3, 4, 5, 6, 10, 11, 12};

    if (!derive_vault_pda(multisig, vault_index, vault) ||
        !derive_program_data_pda(program, program_data)) {
        return false;
    }

    return append_legacy_message(out_message,
                                 out_message_length,
                                 1,
                                 0,
                                 6,
                                 accounts,
                                 sizeof(accounts) / sizeof(accounts[0]),
                                 recent_blockhash,
                                 7,
                                 account_indexes,
                                 sizeof(account_indexes),
                                 G_vault_transaction_execute_discriminator,
                                 sizeof(G_vault_transaction_execute_discriminator));
}

void app_format_short_pubkey(const uint8_t pubkey[PUBKEY_LENGTH], char out[18]) {
    static const char hex[] = "0123456789abcdef";
    for (size_t index = 0; index < 4; index++) {
        out[index * 2] = hex[(pubkey[index] >> 4) & 0x0F];
        out[index * 2 + 1] = hex[pubkey[index] & 0x0F];
    }
    out[8] = '.';
    out[9] = '.';
    for (size_t index = 0; index < 4; index++) {
        out[10 + index * 2] = hex[(pubkey[PUBKEY_LENGTH - 4 + index] >> 4) & 0x0F];
        out[11 + index * 2] = hex[pubkey[PUBKEY_LENGTH - 4 + index] & 0x0F];
    }
    out[17] = '\0';
}

void app_format_path(const uint32_t *path, uint8_t path_length, char *out, size_t out_length) {
    size_t offset = 0;
    if (out_length == 0) {
        return;
    }

    offset += snprintf(out + offset, out_length - offset, "m");
    for (uint8_t index = 0; index < path_length && offset < out_length; index++) {
        uint32_t value = path[index];
        bool hardened = (value & 0x80000000UL) != 0;
        value &= 0x7FFFFFFFUL;
        offset += snprintf(out + offset, out_length - offset, "/%lu%s", (unsigned long) value,
                           hardened ? "'" : "");
    }
}

void app_format_u64(uint64_t value, char *out, size_t out_length) {
    snprintf(out, out_length, "%llu", (unsigned long long) value);
}

void app_format_hex(const uint8_t *bytes, size_t length, char *out, size_t out_length) {
    static const char hex[] = "0123456789abcdef";
    size_t offset = 0;
    for (size_t index = 0; index < length && (offset + 2) < out_length; index++) {
        out[offset++] = hex[(bytes[index] >> 4) & 0x0F];
        out[offset++] = hex[bytes[index] & 0x0F];
    }
    out[offset] = '\0';
}
