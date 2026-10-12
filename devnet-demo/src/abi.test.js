import test from "node:test";
import assert from "node:assert/strict";
import {placeData,cancelData,tradeData,uint,le} from "./abi.js";
test("canonical maker place/cancel encoding",()=>{
  const p=placeData("ask",1,"18446744073709551616","18446744073709551616",100);
  assert.equal(p.length,43);assert.deepEqual([...p.slice(0,3)],[30,0,1]);
  assert.deepEqual([...p.slice(3,11)],[0,0,0,0,0,0,0,0]);
  assert.equal(p[11],1);
  const c=cancelData("bid",0,12);assert.equal(c.length,11);
  assert.deepEqual([...c],[31,1,0,12,0,0,0,0,0,0,0]);
});
test("linked active swap encoding",()=>{
  for(const side of ["ask","bid"])for(const mode of [false,true]){
    const d=tradeData(side,mode,2,100,50);
    assert.equal(d.length,18);assert.equal(d[1],2);
    assert.equal(d[0],side==="ask"?(mode?24:23):(mode?26:25));
    assert.equal(d[2],100);assert.equal(d[10],50);
  }
});
test("reject invalid numerical and page data",()=>{
  for(const v of ["-1","0","1.5","abc",String(1n<<64n)])assert.throws(()=>uint(v,64));
  assert.throws(()=>le(String(1n<<128n),16));
  assert.throws(()=>tradeData("ask",false,9,10,1));
  assert.throws(()=>placeData("ask",2,1,1,1));
});
