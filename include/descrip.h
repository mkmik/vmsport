/* descrip.h: string descriptors, as DEC C has them (vmsport). */
#ifndef __DESCRIP_LOADED
#define __DESCRIP_LOADED 1

#define DSC$K_DTYPE_Z 0
#define DSC$K_DTYPE_T 14
#define DSC$K_CLASS_S 1
#define DSC$K_CLASS_D 2

struct dsc$descriptor {
    unsigned short dsc$w_length;
    unsigned char dsc$b_dtype;
    unsigned char dsc$b_class;
    char *dsc$a_pointer;
};
#define dsc$descriptor_s dsc$descriptor
#define dsc$descriptor_d dsc$descriptor

/* A static descriptor for a string literal. */
#define $DESCRIPTOR(name, string) \
    struct dsc$descriptor_s name = {sizeof(string) - 1, DSC$K_DTYPE_T, DSC$K_CLASS_S, (char *)(string)}

#endif
