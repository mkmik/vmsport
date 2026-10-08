$! RMS's status values and FAB/RAB/XAB/NAM constants, from the DEC C
$! headers in SYS$STARLET_C.TLB: only #define lines (names and values).
$ SET NOON
$ tlb = "SYS$LIBRARY:SYS$STARLET_C.TLB"
$ i = 0
$loop:
$ m = F$ELEMENT(i, ",", "RMSDEF,FABDEF,RABDEF,NAMDEF,XABDEF,XABKEYDEF,XABALLDEF,XABDATDEF,XABFHCDEF,XABPRODEF,XABSUMDEF,XABRDTDEF")
$ IF m .EQS. "," THEN GOTO done
$ LIBRARY/EXTRACT='m'/OUTPUT='m'.H 'tlb'
$ SEARCH/NOHEADING 'm'.H "#define "
$ i = i + 1
$ GOTO loop
$done:
$ EXIT 1
