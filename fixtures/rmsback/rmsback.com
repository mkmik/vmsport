$! What OpenVMS makes of files vms-rms made (ours/): ANALYZE/RMS_FILE,
$! DCL READ, READ/KEY with a record number DCL can type, and changes to
$! RGAPS.DAT made here.
$ SET NOON
$ i = 0
$ana:
$ f = F$ELEMENT(i, ",", "RGAPS,RFIXG,RVFCG,SFTN,SCRBLK,RBADLEN,RBADCTL,SBAD")
$ IF f .EQS. "," THEN GOTO read
$ ANALYZE/RMS_FILE/FDL/OUTPUT='f'.ANL 'f'.DAT
$ ANALYZE/RMS_FILE/CHECK/OUTPUT='f'.CHK 'f'.DAT
$ i = i + 1
$ GOTO ana
$read:
$ i = 0
$readf:
$ f = F$ELEMENT(i, ",", "RGAPS,RFIXG,RVFCG,SFTN,SCRBLK")
$ IF f .EQS. "," THEN GOTO key
$ WRITE SYS$OUTPUT "@@ read ", f
$ OPEN/READ in 'f'.DAT
$readr:
$ READ/END_OF_FILE=reade in r
$ WRITE SYS$OUTPUT "[", r, "]"
$ GOTO readr
$reade:
$ CLOSE in
$ i = i + 1
$ GOTO readf
$key:
$! "A   " is record number %X20202041.
$ WRITE SYS$OUTPUT "@@ key"
$ OPEN/READ in RFIXG.DAT
$ READ/KEY="A   " in r
$ WRITE SYS$OUTPUT F$FAO("RFIXG !XL", F$INTEGER($STATUS))
$ CLOSE in
$ OPEN/READ in RGAPS.DAT
$ READ/KEY="A   " in r
$ WRITE SYS$OUTPUT F$FAO("RGAPS !XL", F$INTEGER($STATUS))
$ READ/KEY="AB" in r
$ WRITE SYS$OUTPUT F$FAO("RGAPS 2 !XL", F$INTEGER($STATUS))
$ CLOSE in
$ WRITE SYS$OUTPUT "@@ change"
$ OPEN/READ/WRITE f RGAPS.DAT
$ READ f r
$ READ/DELETE f r
$ READ f r
$ WRITE/UPDATE f "13 by VMS"
$ CLOSE f
$ OPEN/APPEND f RGAPS.DAT
$ WRITE f "appended on VMS"
$ CLOSE f
$ ANALYZE/RMS_FILE/CHECK/OUTPUT=RGAPSV.CHK RGAPS.DAT
$ OPEN/READ in RGAPS.DAT
$changer:
$ READ/END_OF_FILE=changee in r
$ WRITE SYS$OUTPUT "[", r, "]"
$ GOTO changer
$changee:
$ CLOSE in
$ DIRECTORY/FULL RGAPS.DAT
$ EXIT 1
