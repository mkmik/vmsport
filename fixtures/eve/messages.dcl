SET DEFAULT SYS$SYSDEVICE:[000000]
DIRECTORY/NOHEADING/NOTRAILING SYS$MESSAGE:*TPU*.EXE,*EVE*.EXE
CREATE TPUMSG.COM
$! Prints TPU's (1010) and EVE's (548) messages, code and F$MESSAGE text,
$! with each message file of SYS$MESSAGE that names TPU or EVE, and then
$! TPU's shareable image.
$ SET NOON
$files:
$ f = F$SEARCH("SYS$MESSAGE:*.EXE")
$ IF f .EQS. "" THEN GOTO share
$ name = F$PARSE(f,,,"NAME")
$ IF F$LOCATE("TPU", name) .EQ. F$LENGTH(name) .AND. -
	F$LOCATE("EVE", name) .EQ. F$LENGTH(name) THEN GOTO files
$ CALL try 'f'
$ GOTO files
$share:
$ CALL try SYS$SHARE:TPUSHR.EXE
$ EXIT 1
$try: SUBROUTINE
$ WRITE SYS$OUTPUT "@@ file ", P1
$ SET MESSAGE 'P1'
$ CALL scan 1010
$ CALL scan 548
$ ENDSUBROUTINE
$scan: SUBROUTINE
$ n = 0
$loop:
$ code = F$INTEGER(P1) * %X10000 + %X8000 + n * 8
$ msg = F$MESSAGE(code)
$ IF F$LOCATE("-NOMSG,", msg) .EQ. F$LENGTH(msg) THEN -
	WRITE SYS$OUTPUT F$FAO("!XL", code), " ", msg
$ n = n + 1
$ IF n .LT. 4096 THEN GOTO loop
$ ENDSUBROUTINE
@@CTRLZ
@TPUMSG
