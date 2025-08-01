/*
 * Copyright (c) 2020, Armink, <armink.ztl@gmail.com>
 *
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file
 * @brief utils
 *
 * Some utils for this library.
 */

#include <stdio.h>
#include <string.h>
#include <flashdb.h>
#include <fdb_low_lvl.h>

#define FDB_LOG_TAG "[utils]"

static const uint32_t crc32_table[] =
{
    0x00000000, 0x77073096, 0xee0e612c, 0x990951ba, 0x076dc419, 0x706af48f,
    0xe963a535, 0x9e6495a3, 0x0edb8832, 0x79dcb8a4, 0xe0d5e91e, 0x97d2d988,
    0x09b64c2b, 0x7eb17cbd, 0xe7b82d07, 0x90bf1d91, 0x1db71064, 0x6ab020f2,
    0xf3b97148, 0x84be41de, 0x1adad47d, 0x6ddde4eb, 0xf4d4b551, 0x83d385c7,
    0x136c9856, 0x646ba8c0, 0xfd62f97a, 0x8a65c9ec, 0x14015c4f, 0x63066cd9,
    0xfa0f3d63, 0x8d080df5, 0x3b6e20c8, 0x4c69105e, 0xd56041e4, 0xa2677172,
    0x3c03e4d1, 0x4b04d447, 0xd20d85fd, 0xa50ab56b, 0x35b5a8fa, 0x42b2986c,
    0xdbbbc9d6, 0xacbcf940, 0x32d86ce3, 0x45df5c75, 0xdcd60dcf, 0xabd13d59,
    0x26d930ac, 0x51de003a, 0xc8d75180, 0xbfd06116, 0x21b4f4b5, 0x56b3c423,
    0xcfba9599, 0xb8bda50f, 0x2802b89e, 0x5f058808, 0xc60cd9b2, 0xb10be924,
    0x2f6f7c87, 0x58684c11, 0xc1611dab, 0xb6662d3d, 0x76dc4190, 0x01db7106,
    0x98d220bc, 0xefd5102a, 0x71b18589, 0x06b6b51f, 0x9fbfe4a5, 0xe8b8d433,
    0x7807c9a2, 0x0f00f934, 0x9609a88e, 0xe10e9818, 0x7f6a0dbb, 0x086d3d2d,
    0x91646c97, 0xe6635c01, 0x6b6b51f4, 0x1c6c6162, 0x856530d8, 0xf262004e,
    0x6c0695ed, 0x1b01a57b, 0x8208f4c1, 0xf50fc457, 0x65b0d9c6, 0x12b7e950,
    0x8bbeb8ea, 0xfcb9887c, 0x62dd1ddf, 0x15da2d49, 0x8cd37cf3, 0xfbd44c65,
    0x4db26158, 0x3ab551ce, 0xa3bc0074, 0xd4bb30e2, 0x4adfa541, 0x3dd895d7,
    0xa4d1c46d, 0xd3d6f4fb, 0x4369e96a, 0x346ed9fc, 0xad678846, 0xda60b8d0,
    0x44042d73, 0x33031de5, 0xaa0a4c5f, 0xdd0d7cc9, 0x5005713c, 0x270241aa,
    0xbe0b1010, 0xc90c2086, 0x5768b525, 0x206f85b3, 0xb966d409, 0xce61e49f,
    0x5edef90e, 0x29d9c998, 0xb0d09822, 0xc7d7a8b4, 0x59b33d17, 0x2eb40d81,
    0xb7bd5c3b, 0xc0ba6cad, 0xedb88320, 0x9abfb3b6, 0x03b6e20c, 0x74b1d29a,
    0xead54739, 0x9dd277af, 0x04db2615, 0x73dc1683, 0xe3630b12, 0x94643b84,
    0x0d6d6a3e, 0x7a6a5aa8, 0xe40ecf0b, 0x9309ff9d, 0x0a00ae27, 0x7d079eb1,
    0xf00f9344, 0x8708a3d2, 0x1e01f268, 0x6906c2fe, 0xf762575d, 0x806567cb,
    0x196c3671, 0x6e6b06e7, 0xfed41b76, 0x89d32be0, 0x10da7a5a, 0x67dd4acc,
    0xf9b9df6f, 0x8ebeeff9, 0x17b7be43, 0x60b08ed5, 0xd6d6a3e8, 0xa1d1937e,
    0x38d8c2c4, 0x4fdff252, 0xd1bb67f1, 0xa6bc5767, 0x3fb506dd, 0x48b2364b,
    0xd80d2bda, 0xaf0a1b4c, 0x36034af6, 0x41047a60, 0xdf60efc3, 0xa867df55,
    0x316e8eef, 0x4669be79, 0xcb61b38c, 0xbc66831a, 0x256fd2a0, 0x5268e236,
    0xcc0c7795, 0xbb0b4703, 0x220216b9, 0x5505262f, 0xc5ba3bbe, 0xb2bd0b28,
    0x2bb45a92, 0x5cb36a04, 0xc2d7ffa7, 0xb5d0cf31, 0x2cd99e8b, 0x5bdeae1d,
    0x9b64c2b0, 0xec63f226, 0x756aa39c, 0x026d930a, 0x9c0906a9, 0xeb0e363f,
    0x72076785, 0x05005713, 0x95bf4a82, 0xe2b87a14, 0x7bb12bae, 0x0cb61b38,
    0x92d28e9b, 0xe5d5be0d, 0x7cdcefb7, 0x0bdbdf21, 0x86d3d2d4, 0xf1d4e242,
    0x68ddb3f8, 0x1fda836e, 0x81be16cd, 0xf6b9265b, 0x6fb077e1, 0x18b74777,
    0x88085ae6, 0xff0f6a70, 0x66063bca, 0x11010b5c, 0x8f659eff, 0xf862ae69,
    0x616bffd3, 0x166ccf45, 0xa00ae278, 0xd70dd2ee, 0x4e048354, 0x3903b3c2,
    0xa7672661, 0xd06016f7, 0x4969474d, 0x3e6e77db, 0xaed16a4a, 0xd9d65adc,
    0x40df0b66, 0x37d83bf0, 0xa9bcae53, 0xdebb9ec5, 0x47b2cf7f, 0x30b5ffe9,
    0xbdbdf21c, 0xcabac28a, 0x53b39330, 0x24b4a3a6, 0xbad03605, 0xcdd70693,
    0x54de5729, 0x23d967bf, 0xb3667a2e, 0xc4614ab8, 0x5d681b02, 0x2a6f2b94,
    0xb40bbe37, 0xc30c8ea1, 0x5a05df1b, 0x2d02ef8d
};

/**
 * Calculate the CRC32 value of a memory buffer.
 *
 * @param crc accumulated CRC32 value, must be 0 on first call
 * @param buf buffer to calculate CRC32 value for
 * @param size bytes in buffer
 *
 * @return calculated CRC32 value
 */
uint32_t fdb_calc_crc32(uint32_t crc, const void *buf, size_t size)
{
    const uint8_t *p;

    p = (const uint8_t *)buf;
    crc = crc ^ ~0U;

    while (size--) {
        crc = crc32_table[(crc ^ *p++) & 0xFF] ^ (crc >> 8);
    }

    return crc ^ ~0U;
}

size_t _fdb_set_status(uint8_t status_table[], size_t status_num, size_t status_index)
{
    size_t byte_index = SIZE_MAX;
    /*
     * | write garn |       status0       |       status1       |      status2         |       status3      |
     * ------------------------------------------------------------------------------------------------------
     * |    1bit    | 0xFF                | 0x7F                |  0x3F                |  0x1F
     * ------------------------------------------------------------------------------------------------------
     * |    8bit    | 0xFF FF FF          | 0x00 FF FF          |  0x00 00 FF          |  0x00 00 00
     * ------------------------------------------------------------------------------------------------------
     * |   32bit    | 0xFFFFFFFF FFFFFFFF | 0x00FFFFFF FFFFFFFF |  0x00FFFFFF 00FFFFFF |  0x00FFFFFF 00FFFFFF
     * |            | 0xFFFFFFFF          | 0xFFFFFFFF          |  0xFFFFFFFF          |  0x00FFFFFF
     * ------------------------------------------------------------------------------------------------------
     * |            | 0xFFFFFFFF FFFFFFFF | 0x00FFFFFF FFFFFFFF |  0x00FFFFFF FFFFFFFF |  0x00FFFFFF FFFFFFFF
     * |   64bit    | 0xFFFFFFFF FFFFFFFF | 0xFFFFFFFF FFFFFFFF |  0x00FFFFFF FFFFFFFF |  0x00FFFFFF FFFFFFFF
     * |            | 0xFFFFFFFF FFFFFFFF | 0xFFFFFFFF FFFFFFFF |  0xFFFFFFFF FFFFFFFF |  0x00FFFFFF FFFFFFFF
     */
    memset(status_table, FDB_BYTE_ERASED, FDB_STATUS_TABLE_SIZE(status_num));
    if (status_index > 0) {
#if (FDB_WRITE_GRAN == 1)
        byte_index = (status_index - 1) / 8;
#if (FDB_BYTE_ERASED == 0xFF)
        status_table[byte_index] &= (0x00ff >> (status_index % 8));
#else
        status_table[byte_index] |= (0x00ff >> (status_index % 8));
#endif
#else
        byte_index = (status_index - 1) * (FDB_WRITE_GRAN / 8);
        status_table[byte_index] = FDB_BYTE_WRITTEN;
#endif /* FDB_WRITE_GRAN == 1 */
    }

    return byte_index;
}

size_t _fdb_get_status(uint8_t status_table[], size_t status_num)
{
    size_t i = 0, status_num_bak = --status_num;

    while (status_num --) {
        /* get the first 0 position from end address to start address */
#if (FDB_WRITE_GRAN == 1)
        if ((status_table[status_num / 8] & (0x80 >> (status_num % 8))) == 0x00) {
            break;
        }
#else /*  (FDB_WRITE_GRAN == 8) ||  (FDB_WRITE_GRAN == 32) ||  (FDB_WRITE_GRAN == 64) */
        if (status_table[status_num * FDB_WRITE_GRAN / 8] == FDB_BYTE_WRITTEN) {
            break;
        }
#endif /* FDB_WRITE_GRAN == 1 */
        i++;
    }

    return status_num_bak - i;
}

fdb_err_t _fdb_write_status(fdb_db_t db, uint32_t addr, uint8_t status_table[], size_t status_num, size_t status_index, bool sync)
{
    fdb_err_t result = FDB_NO_ERR;
    size_t byte_index;

    FDB_ASSERT(status_index < status_num);
    FDB_ASSERT(status_table);

    /* set the status first */
    byte_index = _fdb_set_status(status_table, status_num, status_index);

    /* the first status table value is all 1, so no need to write flash */
    if (byte_index == SIZE_MAX) {
        return FDB_NO_ERR;
    }
#if (FDB_WRITE_GRAN == 1)
    result = _fdb_flash_write(db, addr + byte_index, (uint32_t *)&status_table[byte_index], 1, sync);
#else /*  (FDB_WRITE_GRAN == 8) ||  (FDB_WRITE_GRAN == 32) ||  (FDB_WRITE_GRAN == 64) */
    /* write the status by write granularity
     * some flash (like stm32 onchip) NOT supported repeated write before erase */
    result = _fdb_flash_write(db, addr + byte_index, (uint32_t *) &status_table[byte_index], FDB_WRITE_GRAN / 8, sync);
#endif /* FDB_WRITE_GRAN == 1 */

    return result;
}

size_t _fdb_read_status(fdb_db_t db, uint32_t addr, uint8_t status_table[], size_t total_num)
{
    FDB_ASSERT(status_table);

    _fdb_flash_read(db, addr, (uint32_t *) status_table, FDB_STATUS_TABLE_SIZE(total_num));

    return _fdb_get_status(status_table, total_num);
}

/*
 * find the continue 0xFF flash address to end address
 */
uint32_t _fdb_continue_ff_addr(fdb_db_t db, uint32_t start, uint32_t end)
{
    uint8_t buf[32], last_data = FDB_BYTE_WRITTEN;
    size_t i, addr = start, read_size;

    for (; start < end; start += sizeof(buf)) {
        if (start + sizeof(buf) < end) {
            read_size = sizeof(buf);
        } else {
            read_size = end - start;
        }
        _fdb_flash_read(db, start, (uint32_t *) buf, read_size);
        for (i = 0; i < read_size; i++) {
            if (last_data != FDB_BYTE_ERASED && buf[i] == FDB_BYTE_ERASED) {
                addr = start + i;
            }
            last_data = buf[i];
        }
    }

    if (last_data == FDB_BYTE_ERASED) {
        return FDB_WG_ALIGN(addr);
    } else {
        return end;
    }
}

/**
 * Make a blob object.
 *
 * @param blob blob object
 * @param value_buf value buffer
 * @param buf_len buffer length
 *
 * @return new blob object
 */
fdb_blob_t fdb_blob_make(fdb_blob_t blob, const void *value_buf, size_t buf_len)
{
    blob->buf = (void *)value_buf;
    blob->size = buf_len;

    return blob;
}

/**
 * Read the blob object in database.
 *
 * @param db database object
 * @param blob blob object
 *
 * @return read length
 */
size_t fdb_blob_read(fdb_db_t db, fdb_blob_t blob)
{
    size_t read_len = blob->size;

    if (read_len > blob->saved.len) {
        read_len = blob->saved.len;
    }
    if (_fdb_flash_read(db, blob->saved.addr, blob->buf, read_len) != FDB_NO_ERR) {
        read_len = 0;
    }

    return read_len;
}

#ifdef FDB_USING_FILE_MODE
extern fdb_err_t _fdb_file_read(fdb_db_t db, uint32_t addr, void *buf, size_t size);
extern fdb_err_t _fdb_file_write(fdb_db_t db, uint32_t addr, const void *buf, size_t size, bool sync);
extern fdb_err_t _fdb_file_erase(fdb_db_t db, uint32_t addr, size_t size);
#endif /* FDB_USING_FILE_LIBC */

#ifdef FDB_USING_CUSTOM_MODE
extern fdb_err_t fdb_custom_read(fdb_db_t db, uint32_t addr, void *buf, size_t size);
extern fdb_err_t fdb_custom_write(fdb_db_t db, uint32_t addr, const void *buf, size_t size, bool sync);
extern fdb_err_t fdb_custom_erase(fdb_db_t db, uint32_t addr, size_t size);
#endif

fdb_err_t _fdb_flash_read(fdb_db_t db, uint32_t addr, void *buf, size_t size)
{
    switch (db->mode) {
#if defined(FDB_USING_CUSTOM_MODE)
        case FDB_STORAGE_CUSTOM:
            return fdb_custom_read(db, addr, buf, size);
#endif
#if defined(FDB_USING_FILE_MODE)
        case FDB_STORAGE_FILE:
            return _fdb_file_read(db, addr, buf, size);
#endif
#if defined(FDB_USING_FAL_MODE)
        case FDB_STORAGE_FAL:
            if (fal_partition_read(db->storage.part, addr, (uint8_t *) buf, size) < 0) {
                return FDB_READ_ERR;
            }
            return FDB_NO_ERR;
#endif
        default:
            return FDB_READ_ERR;
    }
}

fdb_err_t _fdb_flash_erase(fdb_db_t db, uint32_t addr, size_t size)
{
    switch (db->mode) {
#if defined(FDB_USING_CUSTOM_MODE)
        case FDB_STORAGE_CUSTOM:
            return fdb_custom_erase(db, addr, size);
#endif
#if defined(FDB_USING_FILE_MODE)
        case FDB_STORAGE_FILE:
            return _fdb_file_erase(db, addr, size);
#endif
#if defined(FDB_USING_FAL_MODE)
        case FDB_STORAGE_FAL:
            if (fal_partition_erase(db->storage.part, addr, size) < 0) {
                return FDB_ERASE_ERR;
            }
            return FDB_NO_ERR;
#endif
        default:
            return FDB_ERASE_ERR;
    }
}

fdb_err_t _fdb_flash_write(fdb_db_t db, uint32_t addr, const void *buf, size_t size, bool sync)
{
    switch (db->mode) {
#if defined(FDB_USING_CUSTOM_MODE)
        case FDB_STORAGE_CUSTOM:
            return fdb_custom_write(db, addr, buf, size, sync);
#endif
#if defined(FDB_USING_FILE_MODE)
        case FDB_STORAGE_FILE:
            return _fdb_file_write(db, addr, buf, size, sync);
#endif
#if defined(FDB_USING_FAL_MODE)
        case FDB_STORAGE_FAL:
            if (fal_partition_write(db->storage.part, addr, (const uint8_t *)buf, size) < 0) {
                return FDB_WRITE_ERR;
            }
            return FDB_NO_ERR;
#endif
        default:
            return FDB_WRITE_ERR;
    }
}
