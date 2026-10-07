$! Puts, updates and deletes on the acceptance files: P1 the orders file,
$! P2 the parts file. Run on VMS (fixtures/accept) and on vmsport through
$! a mounted image (crates/vms-utils/tests/accept.rs).
$ SET NOON
$ OPEN/READ/WRITE f 'P1'
$ n = 1
$o_put:
$ k = 310 + n * 41 - (n * 41 / 601) * 601
$ @RECORDS ORDER 'k'
$ WRITE f REC
$ n = n + 1
$ IF n .LE. 600 THEN GOTO o_put
$ n = 7
$o_upd:
$ id = F$FAO("O!7ZL", n)
$ READ/KEY='id' f r
$ r = F$EXTRACT(0, 8, r) + F$FAO("!10AS!4ZL", "NEWCUST", n) + F$EXTRACT(22, 80, r)
$ WRITE/UPDATE f r
$ n = n + 7
$ IF n .LE. 910 THEN GOTO o_upd
$ n = 11
$o_del:
$ id = F$FAO("O!7ZL", n)
$ READ/DELETE/KEY='id' f r
$ n = n + 11
$ IF n .LE. 910 THEN GOTO o_del
$ CLOSE f
$ OPEN/READ/WRITE f 'P2'
$ n = 1
$p_put:
$ k = 210 + n * 29 - (n * 29 / 307) * 307
$ @RECORDS PART 'k'
$ WRITE f REC
$ n = n + 1
$ IF n .LE. 306 THEN GOTO p_put
$ n = 13
$p_del:
$ @RECORDS PART 'n'
$ id = F$EXTRACT(0, 4, REC) + F$EXTRACT(10, 4, REC)
$ READ/DELETE/KEY="''id'" f r
$ n = n + 13
$ IF n .LE. 516 THEN GOTO p_del
$ CLOSE f
