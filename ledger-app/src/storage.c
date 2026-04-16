#include "storage.h"

#include "app_storage.h"

static void make_empty_storage(internal_storage_t *storage) {
    memset(storage, 0, sizeof(*storage));
    storage->initialized = 0x01;
}

static bool read_storage(internal_storage_t *storage) {
    int32_t result = app_storage_read(storage, sizeof(*storage), 0);
    if (result != (int32_t) sizeof(*storage)) {
        make_empty_storage(storage);
        return false;
    }
    return storage->initialized == 0x01;
}

static bool write_storage(const internal_storage_t *storage) {
    int32_t result = app_storage_write(storage, sizeof(*storage), 0);
    if (result != (int32_t) sizeof(*storage)) {
        return false;
    }
    app_storage_increment_data_version();
    return true;
}

void storage_init(void) {
    internal_storage_t storage;
    if (read_storage(&storage)) {
        return;
    }

    make_empty_storage(&storage);
    write_storage(&storage);
}

int storage_find_multisig(const uint8_t multisig[PUBKEY_LENGTH]) {
    internal_storage_t storage;
    if (!read_storage(&storage)) {
        return -1;
    }

    for (uint8_t index = 0; index < MAX_SAVED_MULTISIGS; index++) {
        if (!storage.entries[index].occupied) {
            continue;
        }

        if (memcmp(storage.entries[index].multisig, multisig, PUBKEY_LENGTH) == 0) {
            return index;
        }
    }

    return -1;
}

int storage_find_free_slot(void) {
    internal_storage_t storage;
    if (!read_storage(&storage)) {
        return 0;
    }

    for (uint8_t index = 0; index < MAX_SAVED_MULTISIGS; index++) {
        if (!storage.entries[index].occupied) {
            return index;
        }
    }

    return -1;
}

static int storage_find_free_reviewed_upgrade_slot(void) {
    internal_storage_t storage;
    if (!read_storage(&storage)) {
        return 0;
    }

    for (uint8_t index = 0; index < MAX_REVIEWED_UPGRADE_INTENTS; index++) {
        if (!storage.reviewed_upgrades[index].occupied) {
            return index;
        }
    }

    return -1;
}

bool storage_read_slot(uint8_t slot, saved_multisig_entry_t *entry) {
    internal_storage_t storage;
    if (slot >= MAX_SAVED_MULTISIGS) {
        return false;
    }
    if (!read_storage(&storage)) {
        return false;
    }

    if (!storage.entries[slot].occupied) {
        return false;
    }

    memcpy(entry, &storage.entries[slot], sizeof(saved_multisig_entry_t));
    return true;
}

int storage_upsert(const saved_multisig_entry_t *entry) {
    internal_storage_t updated;
    read_storage(&updated);

    int slot = storage_find_multisig(entry->multisig);
    if (slot < 0) {
        slot = storage_find_free_slot();
    }
    if (slot < 0) {
        return -1;
    }

    updated.entries[slot] = *entry;
    updated.entries[slot].occupied = 1;
    updated.initialized = 0x01;
    if (!write_storage(&updated)) {
        return -1;
    }
    return slot;
}

int storage_find_reviewed_upgrade(const uint8_t multisig[PUBKEY_LENGTH], uint64_t transaction_index) {
    internal_storage_t storage;
    if (!read_storage(&storage)) {
        return -1;
    }

    for (uint8_t index = 0; index < MAX_REVIEWED_UPGRADE_INTENTS; index++) {
        reviewed_upgrade_intent_t *entry = &storage.reviewed_upgrades[index];
        if (!entry->occupied) {
            continue;
        }
        if (entry->transaction_index != transaction_index) {
            continue;
        }
        if (memcmp(entry->multisig, multisig, PUBKEY_LENGTH) == 0) {
            return index;
        }
    }

    return -1;
}

bool storage_read_reviewed_upgrade(uint8_t slot, reviewed_upgrade_intent_t *entry) {
    internal_storage_t storage;
    if (slot >= MAX_REVIEWED_UPGRADE_INTENTS) {
        return false;
    }
    if (!read_storage(&storage)) {
        return false;
    }
    if (!storage.reviewed_upgrades[slot].occupied) {
        return false;
    }

    memcpy(entry, &storage.reviewed_upgrades[slot], sizeof(*entry));
    return true;
}

int storage_upsert_reviewed_upgrade(const reviewed_upgrade_intent_t *entry) {
    internal_storage_t updated;
    read_storage(&updated);

    int slot = storage_find_reviewed_upgrade(entry->multisig, entry->transaction_index);
    if (slot < 0) {
        slot = storage_find_free_reviewed_upgrade_slot();
    }
    if (slot < 0) {
        return -1;
    }

    updated.reviewed_upgrades[slot] = *entry;
    updated.reviewed_upgrades[slot].occupied = 1;
    updated.initialized = 0x01;
    if (!write_storage(&updated)) {
        return -1;
    }
    return slot;
}

void storage_reset(void) {
    internal_storage_t empty;
    app_storage_reset();
    make_empty_storage(&empty);
    write_storage(&empty);
}
