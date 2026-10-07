/* rmsdemo: RMS from C, as on VMS. Makes a relative file and a sequential
 * one in the default directory, writes, reads them in order, by key and
 * by RFA, updates and deletes, shows their attributes through XABs, and
 * lists them with $PARSE and $SEARCH. */
#include <stdio.h>
#include <string.h>
#include <rms.h>
#include <starlet.h>

static unsigned int check(const char *what, unsigned int st) {
    printf("%-22s %08X\n", what, st);
    return st;
}

static void show(struct RAB *rab) {
    printf("  [%.*s] rfa %u.%u\n", rab->rab$w_rsz, rab->rab$l_rbf, rab->rab$l_rfa0, rab->rab$w_rfa4);
}

static void relative(void) {
    struct FAB fab = cc$rms_fab;
    struct RAB rab = cc$rms_rab;
    struct XABFHC fhc = cc$rms_xabfhc;
    struct XABSUM sum = cc$rms_xabsum;
    char buf[64];
    unsigned int n;
    const char *recs[] = {"first", "second", "third"};

    fab.fab$l_fna = "DEMO.REL";
    fab.fab$b_fns = strlen(fab.fab$l_fna);
    fab.fab$b_org = FAB$C_REL;
    fab.fab$b_rfm = FAB$C_VAR;
    fab.fab$b_rat = FAB$M_CR;
    fab.fab$w_mrs = 40;
    fab.fab$l_mrn = 100;
    fab.fab$b_fac = FAB$M_PUT | FAB$M_GET | FAB$M_UPD | FAB$M_DEL;
    check("create relative", sys$create(&fab));
    rab.rab$l_fab = &fab;
    check("connect", sys$connect(&rab));
    for (n = 0; n < 3; n++) {
        rab.rab$l_rbf = (char *)recs[n];
        rab.rab$w_rsz = strlen(recs[n]);
        check("put", sys$put(&rab));
    }
    /* By record number: 10, past the others. */
    n = 10;
    rab.rab$b_rac = RAB$C_KEY;
    rab.rab$l_kbf = (char *)&n;
    rab.rab$b_ksz = sizeof n;
    rab.rab$l_rbf = "tenth";
    rab.rab$w_rsz = 5;
    check("put key 10", sys$put(&rab));
    check("put key 10 again", sys$put(&rab));
    rab.rab$l_rop = RAB$M_UIF;
    rab.rab$l_rbf = "tenth, again";
    rab.rab$w_rsz = 12;
    check("put key 10 UIF", sys$put(&rab));
    rab.rab$l_rop = 0;
    check("close", sys$close(&fab));

    fab.fab$l_xab = &fhc;
    fhc.xab$l_nxt = &sum;
    check("open relative", sys$open(&fab));
    printf("  org %02X rfm %d mrs %d mrn %u bks %d lrl %d ebk %u\n", fab.fab$b_org, fab.fab$b_rfm,
           fab.fab$w_mrs, fab.fab$l_mrn, fab.fab$b_bks, fhc.xab$w_lrl, fhc.xab$l_ebk);
    rab = cc$rms_rab;
    rab.rab$l_fab = &fab;
    rab.rab$l_ubf = buf;
    rab.rab$w_usz = sizeof buf;
    check("connect", sys$connect(&rab));
    while (check("get", sys$get(&rab)) & 1)
        show(&rab);
    n = 2;
    rab.rab$b_rac = RAB$C_KEY;
    rab.rab$l_kbf = (char *)&n;
    rab.rab$b_ksz = sizeof n;
    check("get key 2", sys$get(&rab));
    show(&rab);
    rab.rab$l_rbf = "SECOND";
    rab.rab$w_rsz = 6;
    check("update", sys$update(&rab));
    n = 4;
    rab.rab$l_rop = RAB$M_KGE;
    check("get key GE 4", sys$get(&rab));
    show(&rab);
    check("delete", sys$delete(&rab));
    check("delete again", sys$delete(&rab));
    rab.rab$l_rop = 0;
    check("get key 4", sys$get(&rab));
    rab.rab$b_rac = RAB$C_RFA;
    rab.rab$l_rfa0 = 2;
    rab.rab$w_rfa4 = 0;
    check("get rfa 2", sys$get(&rab));
    show(&rab);
    rab.rab$w_usz = 3;
    check("get, short buffer", sys$get(&rab));
    show(&rab);
    rab.rab$w_usz = sizeof buf;
    rab.rab$b_rac = RAB$C_SEQ;
    check("rewind", sys$rewind(&rab));
    while (check("get", sys$get(&rab)) & 1)
        show(&rab);
    check("disconnect", sys$disconnect(&rab));
    check("get, disconnected", sys$get(&rab));
    check("close", sys$close(&fab));
    check("close again", sys$close(&fab));
}

static void sequential(void) {
    struct FAB fab = cc$rms_fab;
    struct RAB rab = cc$rms_rab;
    char buf[64];

    fab.fab$l_fna = "DEMO";
    fab.fab$b_fns = 4;
    fab.fab$l_dna = ".SEQ";
    fab.fab$b_dns = 4;
    fab.fab$b_rat = FAB$M_CR;
    check("create sequential", sys$create(&fab));
    rab.rab$l_fab = &fab;
    check("connect", sys$connect(&rab));
    rab.rab$l_rbf = "one line";
    rab.rab$w_rsz = 8;
    check("put", sys$put(&rab));
    rab.rab$l_rbf = "and another";
    rab.rab$w_rsz = 11;
    check("put", sys$put(&rab));
    check("close", sys$close(&fab));

    fab.fab$b_fac = FAB$M_GET;
    fab.fab$l_fop = 0;
    check("open sequential", sys$open(&fab));
    printf("  org %02X rfm %d rat %d\n", fab.fab$b_org, fab.fab$b_rfm, fab.fab$b_rat);
    rab = cc$rms_rab;
    rab.rab$l_fab = &fab;
    rab.rab$l_ubf = buf;
    rab.rab$w_usz = sizeof buf;
    check("connect", sys$connect(&rab));
    while (check("get", sys$get(&rab)) & 1)
        show(&rab);
    check("close", sys$close(&fab));

    /* A new version, superseding it, and opening it if it is there. */
    fab.fab$b_fac = FAB$M_PUT;
    check("create again", sys$create(&fab));
    check("close", sys$close(&fab));
    fab.fab$l_fop = FAB$M_SUP;
    check("create SUP", sys$create(&fab));
    check("close", sys$close(&fab));
    fab.fab$l_fop = FAB$M_CIF;
    check("create CIF", sys$create(&fab));
    check("close", sys$close(&fab));
    fab.fab$l_fna = "NEW.SEQ";
    fab.fab$b_fns = 7;
    check("create CIF, new", sys$create(&fab));
    check("close", sys$close(&fab));
    check("erase", sys$erase(&fab));
    check("erase again", sys$erase(&fab));
}

static void listing(void) {
    struct FAB fab = cc$rms_fab;
    struct NAM nam = cc$rms_nam;
    char es[NAM$C_MAXRSS], rs[NAM$C_MAXRSS];

    fab.fab$l_fna = "DEMO.*;*";
    fab.fab$b_fns = strlen(fab.fab$l_fna);
    fab.fab$l_nam = &nam;
    nam.nam$l_esa = es;
    nam.nam$b_ess = sizeof es;
    nam.nam$l_rsa = rs;
    nam.nam$b_rss = sizeof rs;
    check("parse", sys$parse(&fab));
    printf("  fnb %08X name [%.*s%.*s%.*s]\n", nam.nam$l_fnb, nam.nam$b_name, nam.nam$l_name,
           nam.nam$b_type, nam.nam$l_type, nam.nam$b_ver, nam.nam$l_ver);
    while (check("search", sys$search(&fab)) & 1)
        printf("  %.*s%.*s%.*s\n", nam.nam$b_name, nam.nam$l_name, nam.nam$b_type, nam.nam$l_type,
               nam.nam$b_ver, nam.nam$l_ver);
    fab.fab$l_fna = "NOSUCH.*";
    fab.fab$b_fns = strlen(fab.fab$l_fna);
    check("parse", sys$parse(&fab));
    check("search", sys$search(&fab));
}

int main(void) {
    struct FAB bad = cc$rms_fab;
    bad.fab$b_bid = 0;
    check("open, bad FAB", sys$open(&bad));
    relative();
    sequential();
    listing();
    return 0;
}
