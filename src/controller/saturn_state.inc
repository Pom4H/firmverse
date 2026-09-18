/* Snapshot ABI FVS1: dynamic values only. Rebuilt pointer/offset tables are never
 * copied. Exact program identity and payload integrity are checked before writes.
 * This ABI supports the pinned little-endian FBD v11 build; it is not a firmware
 * memory dump nor a generic untrusted-file interface. */
extern tSignal *fbdMemoryBuf, *fbdStorageBuf;
extern char *fbdFlagsBuf, *fbdChangeVarBuf, *fbdChangeModbusBuf;
extern unsigned char *fbdModbusResultBuf;
extern tElemIndex fbdElementsCount, fbdStorageCount, fbdFlagsByteCount;
extern tElemIndex fbdChangeVarByteCount, fbdChangeModbusByteCount, fbdModbusResCount;
extern tElemIndex fbdModbusRTUIndex, fbdModbusTCPIndex, fbdStartEventLog;
extern short fbdModbusRTUTimer, fbdModbusRTUDelayTimer, fbdModbusTCPTimer;
extern short fbdLastScreenIndex, fbdCurrentScreen;
extern unsigned short fbdCurrentScreenTimer;
extern unsigned char fbdModbusRTUErrorCounter, fbdModbusTCPErrorCounter;
extern char fbdFirstFlag;
extern unsigned char fbdEventActiveFlags[32];
extern tEventFlagsView fbdEventActiveTime[256];
extern bool intCurrentEventChanged, intLogEventChanged;
static uint32_t fv_hash(const unsigned char *data, int length) {
    uint32_t h=2166136261u;int i;for(i=0;i<length;i++)h=(h^data[i])*16777619u;return h;
}
typedef struct {void *data;int size;} FvSpan;
static int fv_spans(FvSpan *s) {
    int n=0;
#define FV_SPAN(p,l) do{s[n].data=(void*)(p);s[n++].size=(int)(l);}while(0)
    FV_SPAN(fbdMemoryBuf, fbdElementsCount*sizeof(tSignal));
    FV_SPAN(fbdStorageBuf, fbdStorageCount*sizeof(tSignal));
    FV_SPAN(fbdFlagsBuf, fbdFlagsByteCount);
    FV_SPAN(fbdChangeVarBuf, fbdChangeVarByteCount);
    FV_SPAN(fbdChangeModbusBuf, fbdChangeModbusByteCount);
    FV_SPAN(fbdModbusResultBuf, fbdModbusResCount);
    FV_SPAN(fv_inputs,sizeof(fv_inputs));FV_SPAN(fv_outputs,sizeof(fv_outputs));
    FV_SPAN(fv_hardware,sizeof(fv_hardware));FV_SPAN(fv_nvram,sizeof(fv_nvram));
    FV_SPAN(fbdEventActiveFlags,sizeof(fbdEventActiveFlags));
    FV_SPAN(fbdEventActiveTime,sizeof(fbdEventActiveTime));
#undef FV_SPAN
    return n;
}
#define FV_SCALARS 14
static void fv_get_scalars(int32_t *v) {
    v[0]=fbdModbusRTUIndex;v[1]=fbdModbusTCPIndex;v[2]=fbdModbusRTUTimer;
    v[3]=fbdModbusRTUDelayTimer;v[4]=fbdModbusTCPTimer;v[5]=fbdLastScreenIndex;
    v[6]=fbdCurrentScreen;v[7]=fbdCurrentScreenTimer;v[8]=fbdModbusRTUErrorCounter;
    v[9]=fbdModbusTCPErrorCounter;v[10]=fbdFirstFlag;v[11]=fbdStartEventLog;
    v[12]=intCurrentEventChanged;v[13]=intLogEventChanged;
}
static void fv_set_scalars(const int32_t *v) {
    fbdModbusRTUIndex=(tElemIndex)v[0];fbdModbusTCPIndex=(tElemIndex)v[1];fbdModbusRTUTimer=(short)v[2];
    fbdModbusRTUDelayTimer=(short)v[3];fbdModbusTCPTimer=(short)v[4];fbdLastScreenIndex=(short)v[5];
    fbdCurrentScreen=(short)v[6];fbdCurrentScreenTimer=(unsigned short)v[7];fbdModbusRTUErrorCounter=(unsigned char)v[8];
    fbdModbusTCPErrorCounter=(unsigned char)v[9];fbdFirstFlag=(char)v[10];fbdStartEventLog=(tElemIndex)v[11];
    intCurrentEventChanged=v[12]!=0;intLogEventChanged=v[13]!=0;
}
int fv_fbd_snapshot_size(void) {
    FvSpan spans[12];int i,n,total=16+FV_SCALARS*4;
    if(!fv_memory)return 0;n=fv_spans(spans);for(i=0;i<n;i++)total+=spans[i].size;return total;
}
int fv_fbd_snapshot(unsigned char *out,int capacity) {
    FvSpan spans[12];int32_t scalars[FV_SCALARS];uint32_t header[4];int i,n,at=16,size=fv_fbd_snapshot_size();
    if(!out||!size||capacity<size)return 0;
    header[0]=0x31535646u;header[1]=(uint32_t)size;header[2]=fv_hash(fv_schema,fv_schema_size);header[3]=0;
    fv_get_scalars(scalars);memcpy(out+at,scalars,sizeof(scalars));at+=(int)sizeof(scalars);
    n=fv_spans(spans);for(i=0;i<n;i++){memcpy(out+at,spans[i].data,(size_t)spans[i].size);at+=spans[i].size;}
    header[3]=fv_hash(out+16,size-16);memcpy(out,header,sizeof(header));return size;
}
int fv_fbd_restore(const unsigned char *data,int length) {
    FvSpan spans[12];uint32_t header[4];int32_t scalars[FV_SCALARS];int i,n,at=16;
    if(!data||length<16||length!=fv_fbd_snapshot_size())return 0;
    memcpy(header,data,sizeof(header));
    if(header[0]!=0x31535646u||header[1]!=(uint32_t)length||header[2]!=fv_hash(fv_schema,fv_schema_size)||header[3]!=fv_hash(data+16,length-16))return 0;
    memcpy(scalars,data+at,sizeof(scalars));at+=(int)sizeof(scalars);
    // The runtime uses these values as indexes; reject damaged/malicious counters.
    for(i=0;i<2;i++)if(scalars[i]<0||(scalars[i]>=fbdElementsCount&&scalars[i]!=MAX_INDEX))return 0;
    for(i=2;i<=4;i++)if(scalars[i]<0||scalars[i]>32767)return 0;
    if(scalars[5]<-1||scalars[5]>32767||scalars[6]<-1||scalars[6]>32767||scalars[7]<0||scalars[7]>65535)return 0;
    if(scalars[8]<0||scalars[8]>255||scalars[9]<0||scalars[9]>255||scalars[10]<0||scalars[10]>1||scalars[11]<0||scalars[11]>65535)return 0;
    for(i=12;i<14;i++)if(scalars[i]<0||scalars[i]>1)return 0;
    n=fv_spans(spans);for(i=0;i<n;i++){memcpy(spans[i].data,data+at,(size_t)spans[i].size);at+=spans[i].size;}
    fv_set_scalars(scalars);fv_draw_count=0;return 1;
}
