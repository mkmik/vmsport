/* greet.c: GREET in C. The same command table as the Rust greet, and the
 * same output.
 *
 *     vmsport cdu GREET.CLD -o greet_tables.c
 *     cc -I $VMSPORT/include greet.c greet_tables.c -L <lib> -lvms -o greet
 */
#include <ctype.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <descrip.h>
#include <cli$routines.h>
#include <lib$routines.h>
#include <starlet.h>
#include <ssdef.h>
#include <stsdef.h>

extern char GREET_TABLES[];

static const char *entities[] = {"NAMES", "COUNT", "LOUD", "STYLE", "STYLE.PLAIN", "STYLE.FRAME",
                                 "STYLE.WHISPER", "SIGN", "SYMBOL"};

static unsigned int present(const char *name) {
    struct dsc$descriptor_s d = {strlen(name), DSC$K_DTYPE_T, DSC$K_CLASS_S, (char *)name};
    return cli$present(&d);
}

/* The next value of `name` into buf (NUL-terminated); its status. */
static unsigned int value(const char *name, char *buf, unsigned short size) {
    struct dsc$descriptor_s d = {strlen(name), DSC$K_DTYPE_T, DSC$K_CLASS_S, (char *)name};
    struct dsc$descriptor_s r = {size - 1, DSC$K_DTYPE_T, DSC$K_CLASS_S, buf};
    unsigned short len = 0;
    unsigned int st = cli$get_value(&d, &r, &len);
    buf[$VMS_STATUS_SUCCESS(st) ? len : 0] = 0;
    return st;
}

static void out(const char *line) {
    struct dsc$descriptor_s d = {strlen(line), DSC$K_DTYPE_T, DSC$K_CLASS_S, (char *)line};
    lib$put_output(&d);
}

int main(void) {
    char line[512], buf[256], names[16][64], sign[128] = "", stars[64] = "";
    int n = 0, count = 1, frame = 0, i, j;
    unsigned int st = cli$dcl_parse(0, &GREET_TABLES);
    if (!$VMS_STATUS_SUCCESS(st)) sys$exit(st | STS$M_INHIB_MSG);

    for (i = 0; i < (int)(sizeof entities / sizeof *entities); i++) {
        snprintf(line, sizeof line, "%s %08X", entities[i], present(entities[i]));
        out(line);
    }
    while (n < 16 && $VMS_STATUS_SUCCESS(st = value("NAMES", buf, sizeof buf))) {
        snprintf(line, sizeof line, "  value %s %08X", buf, st);
        out(line);
        strncpy(names[n++], buf, 63);
    }
    struct dsc$descriptor_s fd = {sizeof buf - 1, DSC$K_DTYPE_T, DSC$K_CLASS_S, buf};
    unsigned short flen = 0;
    lib$get_foreign(&fd, 0, &flen);
    buf[flen] = 0;
    snprintf(line, sizeof line, "foreign [%s]", buf);
    out(line);

    if (present("COUNT") & 1 && $VMS_STATUS_SUCCESS(value("COUNT", buf, sizeof buf))) count = atoi(buf);
    if (present("STYLE.FRAME") & 1 && $VMS_STATUS_SUCCESS(value("STYLE.FRAME", buf, sizeof buf))) frame = atoi(buf);
    if (present("SIGN") & 1) value("SIGN", sign, sizeof sign);
    for (i = 0; i < frame && i < 63; i++) stars[i] = '*';

    for (i = 0; i < n; i++) {
        /* NOBODY: a shared message with an FAO argument, %SYSTEM-E-OPENIN. */
        if (strcmp(names[i], "NOBODY") == 0) {
            struct dsc$descriptor_s nd = {strlen(names[i]), DSC$K_DTYPE_T, DSC$K_CLASS_S, names[i]};
            lib$signal(0x109A, 1, &nd);
            continue;
        }
        for (j = 0; j < count; j++) {
            char *p;
            snprintf(buf, sizeof buf, "Hello, %s!%s", names[i], present("LOUD") & 1 ? "!" : "");
            if (present("STYLE.WHISPER") & 1)
                for (p = buf; *p; p++) *p = tolower((unsigned char)*p);
            if (frame)
                snprintf(line, sizeof line, "%s %s %s", stars, buf, stars);
            else
                snprintf(line, sizeof line, "%s", buf);
            if (*sign) strncat(line, " -- ", sizeof line - strlen(line) - 1), strncat(line, sign, sizeof line - strlen(line) - 1);
            out(line);
        }
    }

    if (present("SYMBOL") & 1) {
        char sym[64], num[16];
        value("SYMBOL", sym, sizeof sym);
        snprintf(num, sizeof num, "%d", n * count);
        struct dsc$descriptor_s sd = {strlen(sym), DSC$K_DTYPE_T, DSC$K_CLASS_S, sym};
        struct dsc$descriptor_s vd = {strlen(num), DSC$K_DTYPE_T, DSC$K_CLASS_S, num};
        st = lib$set_symbol(&sd, &vd);
        snprintf(line, sizeof line, "set %s = %s %08X", sym, num, st);
        out(line);
    }
    sys$exit(SS$_NORMAL);
}
