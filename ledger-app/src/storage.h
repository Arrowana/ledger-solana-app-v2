#pragma once

#include "app.h"

void storage_init(void);
int storage_find_multisig(const uint8_t multisig[PUBKEY_LENGTH]);
int storage_find_free_slot(void);
bool storage_read_slot(uint8_t slot, saved_multisig_entry_t *entry);
int storage_upsert(const saved_multisig_entry_t *entry);
int storage_find_reviewed_upgrade(const uint8_t multisig[PUBKEY_LENGTH], uint64_t transaction_index);
bool storage_read_reviewed_upgrade(uint8_t slot, reviewed_upgrade_intent_t *entry);
int storage_upsert_reviewed_upgrade(const reviewed_upgrade_intent_t *entry);
void storage_reset(void);
