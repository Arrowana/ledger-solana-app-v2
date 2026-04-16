#include "app.h"
#include "storage.h"
#include "squads_tx.h"
#include "ui.h"

typedef enum {
    PendingNone = 0,
    PendingSave,
    PendingVote,
    PendingCreateUpgrade,
    PendingExecuteUpgrade,
    PendingReset,
} pending_action_kind_t;

static pending_action_kind_t G_pending_kind = PendingNone;
static saved_multisig_entry_t G_pending_entry;
static reviewed_upgrade_intent_t G_pending_reviewed_upgrade;
static uint8_t G_pending_slot = 0;
static uint8_t G_pending_response[MAX_RESPONSE_LENGTH];
static size_t G_pending_response_length = 0;

static void clear_pending(void) {
    G_pending_kind = PendingNone;
    memset(&G_pending_entry, 0, sizeof(G_pending_entry));
    memset(&G_pending_reviewed_upgrade, 0, sizeof(G_pending_reviewed_upgrade));
    G_pending_slot = 0;
    G_pending_response_length = 0;
    memset(G_pending_response, 0, sizeof(G_pending_response));
}

void app_exit(void) {
    os_sched_exit(-1);
}

int io_send_response(const uint8_t *buffer, size_t length, uint16_t status_word) {
    return io_send_response_pointer(buffer, length, status_word);
}

int io_send_status(uint16_t status_word) {
    return io_send_sw(status_word);
}

void app_approve_pending(void) {
    if (G_pending_kind == PendingSave) {
        uint8_t member[PUBKEY_LENGTH];
        memcpy(member, G_pending_entry.member, PUBKEY_LENGTH);
        int slot = storage_upsert(&G_pending_entry);
        clear_pending();
        if (slot < 0) {
            io_send_status(SW_CONDITIONS_NOT_SATISFIED);
            ui_idle();
            return;
        }

        uint8_t response[33];
        response[0] = (uint8_t) slot;
        memcpy(response + 1, member, PUBKEY_LENGTH);
        io_send_response(response, sizeof(response), SW_OK);
        ui_multisig_modal(true);
        return;
    } else if (G_pending_kind == PendingVote || G_pending_kind == PendingExecuteUpgrade) {
        io_send_response(G_pending_response, G_pending_response_length, SW_OK);
        clear_pending();
        ui_transaction_modal(true);
        return;
    } else if (G_pending_kind == PendingCreateUpgrade) {
        if (storage_upsert_reviewed_upgrade(&G_pending_reviewed_upgrade) < 0) {
            clear_pending();
            io_send_status(SW_CONDITIONS_NOT_SATISFIED);
            ui_idle();
            return;
        }
        io_send_response(G_pending_response, G_pending_response_length, SW_OK);
        clear_pending();
        ui_transaction_modal(true);
        return;
    } else if (G_pending_kind == PendingReset) {
        storage_reset();
        clear_pending();
        io_send_status(SW_OK);
        ui_transaction_modal(true);
        return;
    }

    io_send_status(SW_UNKNOWN);
    ui_idle();
}

void app_reject_pending(void) {
    pending_action_kind_t rejected_kind = G_pending_kind;
    clear_pending();
    io_send_status(SW_USER_REFUSED);
    if (rejected_kind == PendingSave) {
        ui_multisig_modal(false);
        return;
    }
    if (rejected_kind == PendingVote || rejected_kind == PendingCreateUpgrade ||
        rejected_kind == PendingExecuteUpgrade || rejected_kind == PendingReset) {
        ui_transaction_modal(false);
        return;
    }
    ui_idle();
}

static int handle_get_version(void) {
    const uint8_t version[] = {0, 1, 0};
    io_send_response(version, sizeof(version), SW_OK);
    return 0;
}

static int handle_save_multisig(const uint8_t *data, size_t data_length, bool non_confirm) {
    saved_multisig_entry_t entry;
    size_t consumed = 0;
    memset(&entry, 0, sizeof(entry));
    storage_init();

    if (!parse_derivation_path(data,
                               data_length,
                               entry.derivation_path,
                               &entry.path_length,
                               &consumed)) {
        return io_send_status(SW_INVALID_DATA);
    }
    if (data_length != consumed + PUBKEY_LENGTH) {
        return io_send_status(SW_INVALID_DATA);
    }

    memcpy(entry.multisig, data + consumed, PUBKEY_LENGTH);
    if (derive_public_key(entry.member, entry.derivation_path, entry.path_length) != CX_OK) {
        return io_send_status(SW_UNKNOWN);
    }

    if (non_confirm) {
        int slot = storage_upsert(&entry);
        if (slot < 0) {
            return io_send_status(SW_CONDITIONS_NOT_SATISFIED);
        }

        uint8_t response[33];
        response[0] = (uint8_t) slot;
        memcpy(response + 1, entry.member, PUBKEY_LENGTH);
        return io_send_response(response, sizeof(response), SW_OK);
    }

    G_pending_kind = PendingSave;
    G_pending_entry = entry;

    char multisig_short[18];
    char member_short[18];
    char path[64];
    app_format_short_pubkey(entry.multisig, multisig_short);
    app_format_short_pubkey(entry.member, member_short);
    app_format_path(entry.derivation_path, entry.path_length, path, sizeof(path));
    ui_show_save_review(multisig_short, member_short, path);
    return 0;
}

static int handle_list_multisig_slot(uint8_t slot) {
    saved_multisig_entry_t entry;
    uint8_t response[1 + PUBKEY_LENGTH + PUBKEY_LENGTH + 1 + (MAX_DERIVATION_PATH_LENGTH * 4)];
    size_t offset = 0;
    storage_init();

    if (!storage_read_slot(slot, &entry)) {
        return io_send_status(SW_NOT_FOUND);
    }

    response[offset++] = 1;
    memcpy(response + offset, entry.multisig, PUBKEY_LENGTH);
    offset += PUBKEY_LENGTH;
    memcpy(response + offset, entry.member, PUBKEY_LENGTH);
    offset += PUBKEY_LENGTH;
    response[offset++] = entry.path_length;
    for (uint8_t index = 0; index < entry.path_length; index++) {
        response[offset++] = (uint8_t) (entry.derivation_path[index] >> 24);
        response[offset++] = (uint8_t) (entry.derivation_path[index] >> 16);
        response[offset++] = (uint8_t) (entry.derivation_path[index] >> 8);
        response[offset++] = (uint8_t) (entry.derivation_path[index]);
    }

    return io_send_response(response, offset, SW_OK);
}

static uint64_t read_u64_le(const uint8_t *bytes) {
    uint64_t value = 0;
    for (size_t index = 0; index < 8; index++) {
        value |= ((uint64_t) bytes[index]) << (index * 8);
    }
    return value;
}

static int handle_proposal_vote(const uint8_t *data, size_t data_length, bool non_confirm) {
    if (data_length != 74 && data_length != 106) {
        return io_send_status(SW_INVALID_DATA);
    }
    storage_init();

    uint8_t multisig[PUBKEY_LENGTH];
    uint8_t recent_blockhash[BLOCKHASH_LENGTH];
    uint8_t proposal[PUBKEY_LENGTH];
    uint8_t message[MAX_MESSAGE_LENGTH];
    uint8_t message_hash[MESSAGE_HASH_LENGTH];
    uint8_t signature[SIGNATURE_LENGTH];
    uint64_t transaction_index = 0;
    size_t message_length = 0;
    int slot = -1;
    uint8_t vote = 0;

    memcpy(multisig, data, PUBKEY_LENGTH);
    memcpy(recent_blockhash, data + 41, BLOCKHASH_LENGTH);
    transaction_index = read_u64_le(data + 32);
    vote = data[40];
    if (vote != PROPOSAL_VOTE_APPROVE && vote != PROPOSAL_VOTE_REJECT) {
        return io_send_status(SW_INVALID_DATA);
    }

    slot = storage_find_multisig(multisig);
    if (slot < 0) {
        return io_send_status(SW_NOT_FOUND);
    }

    saved_multisig_entry_t entry;
    if (!storage_read_slot((uint8_t) slot, &entry)) {
        return io_send_status(SW_NOT_FOUND);
    }

    uint8_t fee_payer_present = data[73];
    if (fee_payer_present > 1) {
        return io_send_status(SW_INVALID_DATA);
    }
    if (fee_payer_present == 1 && memcmp(data + 74, entry.member, PUBKEY_LENGTH) != 0) {
        return io_send_status(SW_CONDITIONS_NOT_SATISFIED);
    }

    if (!derive_proposal_pda(entry.multisig, transaction_index, proposal)) {
        return io_send_status(SW_UNKNOWN);
    }
    if (!build_proposal_vote_message(entry.member,
                                     entry.multisig,
                                     proposal,
                                     vote,
                                     recent_blockhash,
                                     message,
                                     &message_length)) {
        return io_send_status(SW_UNKNOWN);
    }

    sha256_bytes(message, message_length, message_hash);
    if (sign_message_with_path(entry.derivation_path,
                               entry.path_length,
                               message,
                               message_length,
                               signature) != CX_OK) {
        return io_send_status(SW_UNKNOWN);
    }

    memcpy(G_pending_response, signature, SIGNATURE_LENGTH);
    memcpy(G_pending_response + SIGNATURE_LENGTH, entry.member, PUBKEY_LENGTH);
    memcpy(G_pending_response + SIGNATURE_LENGTH + PUBKEY_LENGTH, proposal, PUBKEY_LENGTH);
    memcpy(G_pending_response + SIGNATURE_LENGTH + PUBKEY_LENGTH + PUBKEY_LENGTH,
           message_hash,
           MESSAGE_HASH_LENGTH);
    G_pending_response_length = SIGNATURE_LENGTH + PUBKEY_LENGTH + PUBKEY_LENGTH +
                                MESSAGE_HASH_LENGTH;

    if (non_confirm) {
        return io_send_response(G_pending_response, G_pending_response_length, SW_OK);
    }

    G_pending_kind = PendingVote;
    char multisig_short[18];
    char action[32];
    char member_short[18];
    char tx_index[32];
    char hash_hex[65];
    app_format_short_pubkey(entry.multisig, multisig_short);
    strlcpy(action,
            vote == PROPOSAL_VOTE_APPROVE ? "Approve vote" : "Reject vote",
            sizeof(action));
    app_format_short_pubkey(entry.member, member_short);
    app_format_u64(transaction_index, tx_index, sizeof(tx_index));
    app_format_hex(message_hash, MESSAGE_HASH_LENGTH, hash_hex, sizeof(hash_hex));
    ui_show_vote_review(multisig_short, action, member_short, tx_index, hash_hex);
    return 0;
}

static int handle_proposal_create_upgrade(const uint8_t *data,
                                          size_t data_length,
                                          bool non_confirm) {
    if (data_length != 201) {
        return io_send_status(SW_INVALID_DATA);
    }
    storage_init();

    uint8_t multisig[PUBKEY_LENGTH];
    uint8_t program[PUBKEY_LENGTH];
    uint8_t buffer[PUBKEY_LENGTH];
    uint8_t spill[PUBKEY_LENGTH];
    uint8_t create_blockhash[BLOCKHASH_LENGTH];
    uint8_t proposal_blockhash[BLOCKHASH_LENGTH];
    uint8_t transaction_pda[PUBKEY_LENGTH];
    uint8_t proposal_pda[PUBKEY_LENGTH];
    uint8_t create_message[MAX_MESSAGE_LENGTH];
    uint8_t proposal_message[MAX_MESSAGE_LENGTH];
    uint8_t create_hash[MESSAGE_HASH_LENGTH];
    uint8_t proposal_hash[MESSAGE_HASH_LENGTH];
    uint8_t intent_hash[MESSAGE_HASH_LENGTH];
    uint8_t create_signature[SIGNATURE_LENGTH];
    uint8_t proposal_signature[SIGNATURE_LENGTH];
    uint64_t transaction_index = 0;
    uint8_t vault_index = 0;
    size_t create_message_length = 0;
    size_t proposal_message_length = 0;

    memcpy(multisig, data, PUBKEY_LENGTH);
    transaction_index = read_u64_le(data + 32);
    vault_index = data[40];
    memcpy(program, data + 41, PUBKEY_LENGTH);
    memcpy(buffer, data + 73, PUBKEY_LENGTH);
    memcpy(spill, data + 105, PUBKEY_LENGTH);
    memcpy(create_blockhash, data + 137, BLOCKHASH_LENGTH);
    memcpy(proposal_blockhash, data + 169, BLOCKHASH_LENGTH);

    int slot = storage_find_multisig(multisig);
    if (slot < 0) {
        return io_send_status(SW_NOT_FOUND);
    }

    saved_multisig_entry_t entry;
    if (!storage_read_slot((uint8_t) slot, &entry)) {
        return io_send_status(SW_NOT_FOUND);
    }

    if (!derive_transaction_pda(multisig, transaction_index, transaction_pda) ||
        !derive_proposal_pda(multisig, transaction_index, proposal_pda) ||
        !build_upgrade_intent_hash(multisig,
                                   vault_index,
                                   program,
                                   buffer,
                                   spill,
                                   intent_hash) ||
        !build_upgrade_create_transaction_message(entry.member,
                                                 multisig,
                                                 transaction_pda,
                                                 vault_index,
                                                 program,
                                                 buffer,
                                                 spill,
                                                 create_blockhash,
                                                 create_message,
                                                 &create_message_length) ||
        !build_proposal_create_message(entry.member,
                                      multisig,
                                      proposal_pda,
                                      transaction_index,
                                      proposal_blockhash,
                                      proposal_message,
                                      &proposal_message_length)) {
        return io_send_status(SW_UNKNOWN);
    }

    sha256_bytes(create_message, create_message_length, create_hash);
    sha256_bytes(proposal_message, proposal_message_length, proposal_hash);

    if (sign_message_with_path(entry.derivation_path,
                               entry.path_length,
                               create_message,
                               create_message_length,
                               create_signature) != CX_OK ||
        sign_message_with_path(entry.derivation_path,
                               entry.path_length,
                               proposal_message,
                               proposal_message_length,
                               proposal_signature) != CX_OK) {
        return io_send_status(SW_UNKNOWN);
    }

    memcpy(G_pending_response, create_signature, SIGNATURE_LENGTH);
    memcpy(G_pending_response + SIGNATURE_LENGTH, proposal_signature, SIGNATURE_LENGTH);
    memcpy(G_pending_response + SIGNATURE_LENGTH + SIGNATURE_LENGTH, intent_hash, MESSAGE_HASH_LENGTH);
    memcpy(G_pending_response + SIGNATURE_LENGTH + SIGNATURE_LENGTH + MESSAGE_HASH_LENGTH,
           create_hash,
           MESSAGE_HASH_LENGTH);
    memcpy(G_pending_response + SIGNATURE_LENGTH + SIGNATURE_LENGTH + MESSAGE_HASH_LENGTH +
               MESSAGE_HASH_LENGTH,
           proposal_hash,
           MESSAGE_HASH_LENGTH);
    G_pending_response_length =
        (SIGNATURE_LENGTH * 2) + (MESSAGE_HASH_LENGTH * 3);

    memset(&G_pending_reviewed_upgrade, 0, sizeof(G_pending_reviewed_upgrade));
    memcpy(G_pending_reviewed_upgrade.multisig, multisig, PUBKEY_LENGTH);
    G_pending_reviewed_upgrade.transaction_index = transaction_index;
    G_pending_reviewed_upgrade.vault_index = vault_index;
    memcpy(G_pending_reviewed_upgrade.program, program, PUBKEY_LENGTH);
    memcpy(G_pending_reviewed_upgrade.buffer, buffer, PUBKEY_LENGTH);
    memcpy(G_pending_reviewed_upgrade.spill, spill, PUBKEY_LENGTH);
    memcpy(G_pending_reviewed_upgrade.intent_hash, intent_hash, MESSAGE_HASH_LENGTH);

    if (non_confirm) {
        if (storage_upsert_reviewed_upgrade(&G_pending_reviewed_upgrade) < 0) {
            memset(&G_pending_reviewed_upgrade, 0, sizeof(G_pending_reviewed_upgrade));
            return io_send_status(SW_CONDITIONS_NOT_SATISFIED);
        }
        memset(&G_pending_reviewed_upgrade, 0, sizeof(G_pending_reviewed_upgrade));
        return io_send_response(G_pending_response, G_pending_response_length, SW_OK);
    }

    G_pending_kind = PendingCreateUpgrade;
    char multisig_short[18];
    char tx_index[32];
    char vault_index_string[8];
    char program_short[18];
    char buffer_short[18];
    char spill_short[18];
    char intent_hash_hex[65];
    char create_hash_hex[65];
    char proposal_hash_hex[65];
    app_format_short_pubkey(multisig, multisig_short);
    app_format_u64(transaction_index, tx_index, sizeof(tx_index));
    snprintf(vault_index_string, sizeof(vault_index_string), "%u", vault_index);
    app_format_short_pubkey(program, program_short);
    app_format_short_pubkey(buffer, buffer_short);
    app_format_short_pubkey(spill, spill_short);
    app_format_hex(intent_hash, MESSAGE_HASH_LENGTH, intent_hash_hex, sizeof(intent_hash_hex));
    app_format_hex(create_hash, MESSAGE_HASH_LENGTH, create_hash_hex, sizeof(create_hash_hex));
    app_format_hex(proposal_hash, MESSAGE_HASH_LENGTH, proposal_hash_hex, sizeof(proposal_hash_hex));
    ui_show_create_upgrade_review(multisig_short,
                                  tx_index,
                                  vault_index_string,
                                  program_short,
                                  buffer_short,
                                  spill_short,
                                  intent_hash_hex,
                                  create_hash_hex,
                                  proposal_hash_hex);
    return 0;
}

static int handle_proposal_execute_upgrade(const uint8_t *data,
                                           size_t data_length,
                                           bool non_confirm) {
    if (data_length != 169) {
        return io_send_status(SW_INVALID_DATA);
    }
    storage_init();

    uint8_t multisig[PUBKEY_LENGTH];
    uint8_t program[PUBKEY_LENGTH];
    uint8_t buffer[PUBKEY_LENGTH];
    uint8_t spill[PUBKEY_LENGTH];
    uint8_t recent_blockhash[BLOCKHASH_LENGTH];
    uint8_t transaction_pda[PUBKEY_LENGTH];
    uint8_t proposal_pda[PUBKEY_LENGTH];
    uint8_t message[MAX_MESSAGE_LENGTH];
    uint8_t message_hash[MESSAGE_HASH_LENGTH];
    uint8_t intent_hash[MESSAGE_HASH_LENGTH];
    uint8_t signature[SIGNATURE_LENGTH];
    uint64_t transaction_index = 0;
    uint8_t vault_index = 0;
    size_t message_length = 0;

    memcpy(multisig, data, PUBKEY_LENGTH);
    transaction_index = read_u64_le(data + 32);
    vault_index = data[40];
    memcpy(program, data + 41, PUBKEY_LENGTH);
    memcpy(buffer, data + 73, PUBKEY_LENGTH);
    memcpy(spill, data + 105, PUBKEY_LENGTH);
    memcpy(recent_blockhash, data + 137, BLOCKHASH_LENGTH);

    int slot = storage_find_multisig(multisig);
    if (slot < 0) {
        return io_send_status(SW_NOT_FOUND);
    }

    saved_multisig_entry_t entry;
    reviewed_upgrade_intent_t reviewed;
    int reviewed_slot = storage_find_reviewed_upgrade(multisig, transaction_index);
    if (!storage_read_slot((uint8_t) slot, &entry)) {
        return io_send_status(SW_NOT_FOUND);
    }
    if (reviewed_slot < 0 ||
        !storage_read_reviewed_upgrade((uint8_t) reviewed_slot, &reviewed)) {
        return io_send_status(SW_NOT_FOUND);
    }

    if (!derive_transaction_pda(multisig, transaction_index, transaction_pda) ||
        !derive_proposal_pda(multisig, transaction_index, proposal_pda) ||
        !build_upgrade_intent_hash(multisig,
                                   vault_index,
                                   program,
                                   buffer,
                                   spill,
                                   intent_hash)) {
        return io_send_status(SW_UNKNOWN);
    }

    if (reviewed.vault_index != vault_index ||
        memcmp(reviewed.program, program, PUBKEY_LENGTH) != 0 ||
        memcmp(reviewed.buffer, buffer, PUBKEY_LENGTH) != 0 ||
        memcmp(reviewed.spill, spill, PUBKEY_LENGTH) != 0 ||
        memcmp(reviewed.intent_hash, intent_hash, MESSAGE_HASH_LENGTH) != 0) {
        return io_send_status(SW_CONDITIONS_NOT_SATISFIED);
    }

    if (!build_upgrade_execute_message(entry.member,
                                       multisig,
                                       proposal_pda,
                                       transaction_pda,
                                       vault_index,
                                       program,
                                       buffer,
                                       spill,
                                       recent_blockhash,
                                       message,
                                       &message_length)) {
        return io_send_status(SW_UNKNOWN);
    }

    sha256_bytes(message, message_length, message_hash);
    if (sign_message_with_path(entry.derivation_path,
                               entry.path_length,
                               message,
                               message_length,
                               signature) != CX_OK) {
        return io_send_status(SW_UNKNOWN);
    }

    memcpy(G_pending_response, signature, SIGNATURE_LENGTH);
    memcpy(G_pending_response + SIGNATURE_LENGTH, intent_hash, MESSAGE_HASH_LENGTH);
    memcpy(G_pending_response + SIGNATURE_LENGTH + MESSAGE_HASH_LENGTH,
           message_hash,
           MESSAGE_HASH_LENGTH);
    G_pending_response_length = SIGNATURE_LENGTH + (MESSAGE_HASH_LENGTH * 2);

    if (non_confirm) {
        return io_send_response(G_pending_response, G_pending_response_length, SW_OK);
    }

    G_pending_kind = PendingExecuteUpgrade;
    char multisig_short[18];
    char tx_index[32];
    char vault_index_string[8];
    char program_short[18];
    char buffer_short[18];
    char spill_short[18];
    char intent_hash_hex[65];
    char message_hash_hex[65];
    app_format_short_pubkey(multisig, multisig_short);
    app_format_u64(transaction_index, tx_index, sizeof(tx_index));
    snprintf(vault_index_string, sizeof(vault_index_string), "%u", vault_index);
    app_format_short_pubkey(program, program_short);
    app_format_short_pubkey(buffer, buffer_short);
    app_format_short_pubkey(spill, spill_short);
    app_format_hex(intent_hash, MESSAGE_HASH_LENGTH, intent_hash_hex, sizeof(intent_hash_hex));
    app_format_hex(message_hash, MESSAGE_HASH_LENGTH, message_hash_hex, sizeof(message_hash_hex));
    ui_show_execute_upgrade_review(multisig_short,
                                   tx_index,
                                   vault_index_string,
                                   program_short,
                                   buffer_short,
                                   spill_short,
                                   intent_hash_hex,
                                   message_hash_hex);
    return 0;
}

static int handle_reset(bool non_confirm) {
    storage_init();
    if (non_confirm) {
        storage_reset();
        return io_send_status(SW_OK);
    }

    G_pending_kind = PendingReset;
    ui_show_reset_review();
    return 0;
}

static int handle_apdu(size_t rx) {
    if (rx < APDU_OFFSET_CDATA) {
        return io_send_status(SW_INVALID_DATA);
    }
    if (G_io_apdu_buffer[APDU_OFFSET_CLA] != APP_CLA) {
        return io_send_status(SW_CLA_NOT_SUPPORTED);
    }

    uint8_t ins = G_io_apdu_buffer[APDU_OFFSET_INS];
    uint8_t p1 = G_io_apdu_buffer[APDU_OFFSET_P1];
    uint8_t p2 = G_io_apdu_buffer[APDU_OFFSET_P2];
    uint8_t lc = G_io_apdu_buffer[APDU_OFFSET_LC];
    bool non_confirm = (p1 == P1_NON_CONFIRM);

    if (rx != (size_t) APDU_OFFSET_CDATA + lc) {
        return io_send_status(SW_INVALID_DATA);
    }

    const uint8_t *data = G_io_apdu_buffer + APDU_OFFSET_CDATA;

    switch (ins) {
        case INS_GET_VERSION:
            return handle_get_version();
        case INS_SAVE_MULTISIG:
            return handle_save_multisig(data, lc, non_confirm);
        case INS_LIST_MULTISIG_SLOT:
            return handle_list_multisig_slot(p2);
        case INS_PROPOSAL_VOTE:
            return handle_proposal_vote(data, lc, non_confirm);
        case INS_RESET_MULTISIGS:
            return handle_reset(non_confirm);
        case INS_PROPOSAL_CREATE_UPGRADE:
            return handle_proposal_create_upgrade(data, lc, non_confirm);
        case INS_PROPOSAL_EXECUTE_UPGRADE:
            return handle_proposal_execute_upgrade(data, lc, non_confirm);
        default:
            return io_send_status(SW_INS_NOT_SUPPORTED);
    }
}

void app_main(void) {
    io_init();
    ui_idle();

    for (;;) {
        int rx = io_recv_command();
        if (rx < 0) {
            return;
        }
        handle_apdu((size_t) rx);
    }
}
