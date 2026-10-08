$! Prints VMS's facility names: a shared message (no facility-specific bit)
$! shows the name of the facility in its code.
$ SET NOON
$ fac = 0
$loop:
$ msg = F$MESSAGE(fac * %X10000 + 1)
$ name = F$ELEMENT(0, "-", msg) - "%"
$ IF name .NES. "NONAME" THEN WRITE SYS$OUTPUT fac, " ", name
$ fac = fac + 1
$ IF fac .LT. 4096 THEN GOTO loop
$ EXIT 1
