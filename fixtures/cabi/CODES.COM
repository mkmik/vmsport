$! The status values vmsport's C headers use, from the DEC C headers in
$! SYS$STARLET_C.TLB: only the #define lines of the names wanted.
$ SET NOON
$ tlb = "SYS$LIBRARY:SYS$STARLET_C.TLB"
$ LIBRARY/EXTRACT=LIBDEF/OUTPUT=LIBDEF.H 'tlb'
$ LIBRARY/EXTRACT=SSDEF/OUTPUT=SSDEF.H 'tlb'
$ LIBRARY/EXTRACT=CLIMSGDEF/OUTPUT=CLIMSGDEF.H 'tlb'
$ LIBRARY/EXTRACT=STSDEF/OUTPUT=STSDEF.H 'tlb'
$ LIBRARY/EXTRACT=DSCDEF/OUTPUT=DSCDEF.H 'tlb'
$ LIBRARY/EXTRACT=LCKDEF/OUTPUT=LCKDEF.H 'tlb'
$ LIBRARY/EXTRACT=LKSBDEF/OUTPUT=LKSBDEF.H 'tlb'
$ SEARCH/NOHEADING LIBDEF.H "#define LIB$_"
$ SEARCH/NOHEADING SSDEF.H "#define SS$_NORMAL ","#define SS$_ABORT ", -
	"#define SS$_ACCVIO ","#define SS$_BADPARAM ","#define SS$_BUFFEROVF ", -
	"#define SS$_EXQUOTA ","#define SS$_INSFARG ","#define SS$_INSFMEM ", -
	"#define SS$_IVCHAN ","#define SS$_IVLOGNAM ","#define SS$_NOLOGNAM ", -
	"#define SS$_NOPRIV ","#define SS$_NOSUCHFILE ","#define SS$_NOSUCHDEV ", -
	"#define SS$_SUPERSEDE ","#define SS$_WASSET ","#define SS$_WASCLR ", -
	"#define SS$_CONTROLC ","#define SS$_CONTROLY ","#define SS$_ENDOFFILE ", -
	"#define SS$_TIMEOUT ","#define SS$_CREATED ","#define SS$_DUPLNAM ", -
	"#define SS$_IVSTSFLG ","#define SS$_RESIGNAL ","#define SS$_UNWIND ", -
	"#define SS$_CONTINUE ","#define SS$_NOTRAN ","#define SS$_BADESCAPE ", -
	"#define SS$_MSGNOTFND ","#define SS$_SYNCH ","#define SS$_NOTQUEUED ", -
	"#define SS$_IVLOCKID ","#define SS$_CVTUNGRANT ","#define SS$_DEADLOCK ", -
	"#define SS$_IVRESNAM ","#define SS$_VALNOTVALID ","#define SS$_EXENQLM ", -
	"#define SS$_IVBUFLEN "
$ SEARCH/NOHEADING CLIMSGDEF.H "#define CLI$_"
$ SEARCH/NOHEADING STSDEF.H "#define STS$"
$ SEARCH/NOHEADING DSCDEF.H "#define DSC$K_DTYPE_T ","#define DSC$K_CLASS_S ", -
	"#define DSC$K_CLASS_D ","#define DSC$K_DTYPE_Z "
$ SEARCH/NOHEADING LCKDEF.H "#define LCK$"
$ SEARCH/NOHEADING LKSBDEF.H "#define LKSB$"
$ EXIT 1
