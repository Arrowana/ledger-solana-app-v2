#include "ui.h"

#include <stdio.h>

#include "glyphs.h"
#include "nbgl_use_case.h"
#include "squads_tx.h"
#include "storage.h"

#define INFO_NB 2
#define SETTINGS_INFO_NB (MAX_SAVED_MULTISIGS + 1)
#define SLOT_LABEL_LENGTH 8
#define SLOT_VALUE_LENGTH 48

static char G_review_multisig[32];
static char G_review_action[32];
static char G_review_member[32];
static char G_review_path[64];
static char G_review_index[32];
static char G_review_vault_index[16];
static char G_review_program[32];
static char G_review_buffer[32];
static char G_review_spill[32];
static char G_review_hash[65];
static char G_review_hash_two[65];
static char G_review_hash_three[65];
static nbgl_contentTagValue_t G_review_pairs[9];
static nbgl_contentTagValueList_t G_review_list;
static char G_settings_saved_count[16];
static char G_settings_slot_labels[MAX_SAVED_MULTISIGS][SLOT_LABEL_LENGTH];
static char G_settings_slot_values[MAX_SAVED_MULTISIGS][SLOT_VALUE_LENGTH];
static const char *const G_info_types[INFO_NB] = {
    "Version",
    "Developer",
};
static const char *const G_info_contents[INFO_NB] = {
    APPVERSION,
    "OpenAI",
};
static const nbgl_contentInfoList_t G_info_list = {
    .nbInfos = INFO_NB,
    .infoTypes = G_info_types,
    .infoContents = G_info_contents,
};
static const char *G_settings_types[SETTINGS_INFO_NB];
static const char *G_settings_contents[SETTINGS_INFO_NB];
static nbgl_content_t G_settings_content[1];
static const nbgl_genericContents_t G_settings_contents_list = {
    .callbackCallNeeded = false,
    .contentsList = G_settings_content,
    .nbContents = 1,
};

static void quit_app_callback(void) {
    app_exit();
}

static void fill_saved_multisig_settings(void) {
    saved_multisig_entry_t entry;
    uint8_t saved_count = 0;

    storage_init();

    G_settings_types[0] = "Saved slots";
    G_settings_contents[0] = G_settings_saved_count;

    for (uint8_t slot = 0; slot < MAX_SAVED_MULTISIGS; slot++) {
        snprintf(G_settings_slot_labels[slot], sizeof(G_settings_slot_labels[slot]), "Slot %u", slot);
        G_settings_types[slot + 1] = G_settings_slot_labels[slot];

        if (!storage_read_slot(slot, &entry)) {
            strlcpy(G_settings_slot_values[slot], "Empty", sizeof(G_settings_slot_values[slot]));
            G_settings_contents[slot + 1] = G_settings_slot_values[slot];
            continue;
        }

        char multisig_short[18];
        char member_short[18];
        saved_count++;
        app_format_short_pubkey(entry.multisig, multisig_short);
        app_format_short_pubkey(entry.member, member_short);
        snprintf(G_settings_slot_values[slot],
                 sizeof(G_settings_slot_values[slot]),
                 "%s / %s",
                 multisig_short,
                 member_short);
        G_settings_contents[slot + 1] = G_settings_slot_values[slot];
    }

    snprintf(G_settings_saved_count, sizeof(G_settings_saved_count), "%u/%u", saved_count, MAX_SAVED_MULTISIGS);

    G_settings_content[0].type = INFOS_LIST;
    G_settings_content[0].content.infosList.nbInfos = SETTINGS_INFO_NB;
    G_settings_content[0].content.infosList.infoTypes = G_settings_types;
    G_settings_content[0].content.infosList.infoContents = G_settings_contents;
    G_settings_content[0].content.infosList.infoExtensions = NULL;
    G_settings_content[0].content.infosList.token = 0;
    G_settings_content[0].content.infosList.withExtensions = false;
    G_settings_content[0].contentActionCallback = NULL;
}

static void ui_review_choice(bool approved) {
    if (approved) {
        app_approve_pending();
    } else {
        app_reject_pending();
    }
}

static nbgl_contentTagValueList_t *make_review_list(uint8_t count) {
    G_review_list.pairs = G_review_pairs;
    G_review_list.callback = NULL;
    G_review_list.nbPairs = count;
    G_review_list.startIndex = 0;
    G_review_list.hideEndOfLastLine = false;
    G_review_list.nbMaxLinesForValue = 0;
    G_review_list.token = 0;
    G_review_list.smallCaseForValue = true;
    G_review_list.wrapping = true;
    G_review_list.actionCallback = NULL;
    return &G_review_list;
}

static void ui_main_menu(uint8_t page) {
    fill_saved_multisig_settings();
    nbgl_useCaseHomeAndSettings(APPNAME,
                                &ICON_HOME,
                                NULL,
                                page,
                                &G_settings_contents_list,
                                &G_info_list,
                                NULL,
                                quit_app_callback);
}

void ui_idle(void) {
    ui_main_menu(INIT_HOME_PAGE);
}

void ui_settings(void) {
    ui_main_menu(0);
}

void ui_multisig_modal(bool is_success) {
    if (is_success) {
        nbgl_useCaseReviewStatus(STATUS_TYPE_ADDRESS_VERIFIED, ui_idle);
    } else {
        nbgl_useCaseReviewStatus(STATUS_TYPE_ADDRESS_REJECTED, ui_idle);
    }
}

void ui_transaction_modal(bool is_success) {
    if (is_success) {
        nbgl_useCaseReviewStatus(STATUS_TYPE_TRANSACTION_SIGNED, ui_idle);
    } else {
        nbgl_useCaseReviewStatus(STATUS_TYPE_TRANSACTION_REJECTED, ui_idle);
    }
}

void ui_show_save_review(const char *multisig, const char *member, const char *path) {
    strlcpy(G_review_multisig, multisig, sizeof(G_review_multisig));
    strlcpy(G_review_member, member, sizeof(G_review_member));
    strlcpy(G_review_path, path, sizeof(G_review_path));

    G_review_pairs[0] = (nbgl_contentTagValue_t) {.item = "Multisig", .value = G_review_multisig};
    G_review_pairs[1] = (nbgl_contentTagValue_t) {.item = "Member", .value = G_review_member};
    G_review_pairs[2] = (nbgl_contentTagValue_t) {.item = "Path", .value = G_review_path};

    nbgl_useCaseReview(TYPE_OPERATION,
                       make_review_list(3),
                       &ICON_HOME,
                       "Save multisig",
                       NULL,
                       "Save multisig",
                       ui_review_choice);
}

void ui_show_vote_review(const char *multisig,
                         const char *action,
                         const char *member,
                         const char *transaction_index,
                         const char *message_hash) {
    strlcpy(G_review_multisig, multisig, sizeof(G_review_multisig));
    strlcpy(G_review_action, action, sizeof(G_review_action));
    strlcpy(G_review_member, member, sizeof(G_review_member));
    strlcpy(G_review_index, transaction_index, sizeof(G_review_index));
    strlcpy(G_review_hash, message_hash, sizeof(G_review_hash));

    G_review_pairs[0] = (nbgl_contentTagValue_t) {.item = "Multisig", .value = G_review_multisig};
    G_review_pairs[1] = (nbgl_contentTagValue_t) {.item = "Member", .value = G_review_member};
    G_review_pairs[2] = (nbgl_contentTagValue_t) {.item = "Tx index", .value = G_review_index};
    G_review_pairs[3] = (nbgl_contentTagValue_t) {.item = "Msg hash", .value = G_review_hash};

    nbgl_useCaseReview(TYPE_OPERATION,
                       make_review_list(4),
                       &ICON_SIGN_MENU,
                       G_review_action,
                       NULL,
                       G_review_action,
                       ui_review_choice);
}

void ui_show_reset_review(void) {
    nbgl_useCaseChoice(&ICON_WARNING,
                       "Reset multisigs",
                       "Clear all saved slots",
                       "Reset",
                       "Cancel",
                       ui_review_choice);
}

void ui_show_create_upgrade_review(const char *multisig,
                                   const char *transaction_index,
                                   const char *vault_index,
                                   const char *program,
                                   const char *buffer,
                                   const char *spill,
                                   const char *intent_hash,
                                   const char *transaction_hash,
                                   const char *proposal_hash) {
    strlcpy(G_review_multisig, multisig, sizeof(G_review_multisig));
    strlcpy(G_review_index, transaction_index, sizeof(G_review_index));
    strlcpy(G_review_vault_index, vault_index, sizeof(G_review_vault_index));
    strlcpy(G_review_program, program, sizeof(G_review_program));
    strlcpy(G_review_buffer, buffer, sizeof(G_review_buffer));
    strlcpy(G_review_spill, spill, sizeof(G_review_spill));
    strlcpy(G_review_hash, intent_hash, sizeof(G_review_hash));
    strlcpy(G_review_hash_two, transaction_hash, sizeof(G_review_hash_two));
    strlcpy(G_review_hash_three, proposal_hash, sizeof(G_review_hash_three));

    G_review_pairs[0] = (nbgl_contentTagValue_t) {.item = "Multisig", .value = G_review_multisig};
    G_review_pairs[1] = (nbgl_contentTagValue_t) {.item = "Tx index", .value = G_review_index};
    G_review_pairs[2] = (nbgl_contentTagValue_t) {.item = "Vault", .value = G_review_vault_index};
    G_review_pairs[3] = (nbgl_contentTagValue_t) {.item = "Program", .value = G_review_program};
    G_review_pairs[4] = (nbgl_contentTagValue_t) {.item = "Buffer", .value = G_review_buffer};
    G_review_pairs[5] = (nbgl_contentTagValue_t) {.item = "Spill", .value = G_review_spill};
    G_review_pairs[6] = (nbgl_contentTagValue_t) {.item = "Intent hash", .value = G_review_hash};
    G_review_pairs[7] =
        (nbgl_contentTagValue_t) {.item = "Create hash", .value = G_review_hash_two};
    G_review_pairs[8] =
        (nbgl_contentTagValue_t) {.item = "Proposal hash", .value = G_review_hash_three};

    nbgl_useCaseReview(TYPE_OPERATION,
                       make_review_list(9),
                       &ICON_SIGN_MENU,
                       "Create upgrade",
                       NULL,
                       "Create upgrade",
                       ui_review_choice);
}

void ui_show_execute_upgrade_review(const char *multisig,
                                    const char *transaction_index,
                                    const char *vault_index,
                                    const char *program,
                                    const char *buffer,
                                    const char *spill,
                                    const char *intent_hash,
                                    const char *message_hash) {
    strlcpy(G_review_multisig, multisig, sizeof(G_review_multisig));
    strlcpy(G_review_index, transaction_index, sizeof(G_review_index));
    strlcpy(G_review_vault_index, vault_index, sizeof(G_review_vault_index));
    strlcpy(G_review_program, program, sizeof(G_review_program));
    strlcpy(G_review_buffer, buffer, sizeof(G_review_buffer));
    strlcpy(G_review_spill, spill, sizeof(G_review_spill));
    strlcpy(G_review_hash, intent_hash, sizeof(G_review_hash));
    strlcpy(G_review_hash_two, message_hash, sizeof(G_review_hash_two));

    G_review_pairs[0] = (nbgl_contentTagValue_t) {.item = "Multisig", .value = G_review_multisig};
    G_review_pairs[1] = (nbgl_contentTagValue_t) {.item = "Tx index", .value = G_review_index};
    G_review_pairs[2] = (nbgl_contentTagValue_t) {.item = "Vault", .value = G_review_vault_index};
    G_review_pairs[3] = (nbgl_contentTagValue_t) {.item = "Program", .value = G_review_program};
    G_review_pairs[4] = (nbgl_contentTagValue_t) {.item = "Buffer", .value = G_review_buffer};
    G_review_pairs[5] = (nbgl_contentTagValue_t) {.item = "Spill", .value = G_review_spill};
    G_review_pairs[6] = (nbgl_contentTagValue_t) {.item = "Intent hash", .value = G_review_hash};
    G_review_pairs[7] =
        (nbgl_contentTagValue_t) {.item = "Execute hash", .value = G_review_hash_two};

    nbgl_useCaseReview(TYPE_OPERATION,
                       make_review_list(8),
                       &ICON_SIGN_MENU,
                       "Execute upgrade",
                       NULL,
                       "Execute upgrade",
                       ui_review_choice);
}
