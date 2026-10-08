$! The image vmsport wrote back, as VMS sees it: ANALYZE/RMS_FILE on the
$! files in it, every record by every key, then VMS's own puts and deletes
$! on the copies BACKUP made, checked again.
$ SET NOON
$ on = "DKA100:[T.ACCEPTBACK]"
$ ANALYZE/RMS_FILE/CHECK/OUTPUT=ORDERS.CHK 'on'ORDERS.DAT
$ SHOW SYMBOL $STATUS
$ ANALYZE/RMS_FILE/CHECK/OUTPUT=PARTS.CHK 'on'PARTS.DAT
$ SHOW SYMBOL $STATUS
$ ANALYZE/RMS_FILE/FDL/OUTPUT=ORDERS.ANL 'on'ORDERS.DAT
$ ANALYZE/RMS_FILE/FDL/OUTPUT=PARTS.ANL 'on'PARTS.DAT
$ @READS 'on'ORDERS.DAT 3
$ @READS 'on'PARTS.DAT 2
$ WRITE SYS$OUTPUT "@@ more"
$ OPEN/READ/WRITE f ORDERS.DAT
$ n = 1
$o_put:
$ WRITE f F$FAO("X!7ZL!10AS!4ZLput by VMS", n * 13 - (n * 13 / 401) * 401, "VMS", n)
$ n = n + 1
$ IF n .LE. 400 THEN GOTO o_put
$ n = 5
$o_del:
$ id = F$FAO("O!7ZL", n)
$ READ/DELETE/KEY='id'/ERROR=o_next f rec
$o_next:
$ n = n + 5
$ IF n .LE. 910 THEN GOTO o_del
$ CLOSE f
$ OPEN/READ/WRITE f PARTS.DAT
$ n = 1
$p_put:
$ WRITE f F$FAO("X!3ZLLOC999!4ZL!26AS", n - (n / 9) * 9, n * 17 - (n * 17 / 307) * 307, "put by VMS")
$ n = n + 1
$ IF n .LE. 306 THEN GOTO p_put
$ CLOSE f
$ ANALYZE/RMS_FILE/CHECK/OUTPUT=ORDERS2.CHK ORDERS.DAT
$ SHOW SYMBOL $STATUS
$ ANALYZE/RMS_FILE/CHECK/OUTPUT=PARTS2.CHK PARTS.DAT
$ SHOW SYMBOL $STATUS
