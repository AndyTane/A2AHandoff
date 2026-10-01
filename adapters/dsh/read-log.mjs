import { readFileSync } from 'node:fs';
import { zstdDecompressSync } from 'node:zlib';
// Read-only structural frame traversal; never repair or write the source log.
export function* readRows(file) {
 const b=readFileSync(file); let o=0,rest='';
 while(o<b.length){
  const start=o;
  if(o+5>b.length)throw new Error('Incomplete zstd frame');
  if(b.readUInt32LE(o)!==0xfd2fb528)throw new Error('Invalid zstd frame at '+o);
  o+=4;const h=b[o++];if(h&24)throw new Error('Reserved frame bits');
  const single=!!(h&32),size=h>>>6,dict=h&3;
  o+=(single?0:1)+(dict===3?4:dict)+(size===0?(single?1:0):(1<<size));
  if(o>b.length)throw new Error('Incomplete frame header');
  for(;;){
   if(o+3>b.length)throw new Error('Incomplete block');
   const bh=b.readUIntLE(o,3);o+=3;const type=(bh>>>1)&3;
   if(type===3)throw new Error('Reserved block type');
   o+=type===1?1:(bh>>>3);if(o>b.length)throw new Error('Incomplete block data');
   if(bh&1)break;
  }
  if(h&4)o+=4;if(o>b.length)throw new Error('Incomplete checksum');
  rest+=zstdDecompressSync(b.subarray(start,o)).toString('utf8');
  let end;while((end=rest.indexOf('\n'))>=0){const line=rest.slice(0,end);rest=rest.slice(end+1);if(line.trim())yield JSON.parse(line);}
 }
 if(rest.trim())throw new Error('Incomplete JSONL line');
}
