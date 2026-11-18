import test from "node:test";
import assert from "node:assert/strict";
import {portLabel, deviceLabel, silentReconnectTarget} from "./btPanelModel.ts";

test("usb ports keep the CH340 badge and bluetooth ports say bluetooth", () => {
  assert.equal(
    portLabel({name:"COM13", description:"USB-SERIAL CH340 1A86:7523", preferred:true, kind:"usb"}),
    "COM13 · CH340 USB-SERIAL CH340 1A86:7523"
  );
  assert.equal(
    portLabel({name:"COM7", description:"蓝牙串口 · 70:4B:CA:72:AC:A2", preferred:true, kind:"bluetooth", mac:"70:4B:CA:72:AC:A2"}),
    "COM7 · 蓝牙串口 · 70:4B:CA:72:AC:A2"
  );
  assert.equal(
    portLabel({name:"COM6", description:"Serial port", preferred:false, kind:"other"}),
    "COM6 Serial port"
  );
});

test("legacy ports without a kind still render", () => {
  assert.equal(portLabel({name:"COM5", description:"Serial port", preferred:false}), "COM5 Serial port");
});

test("bluetooth devices show pairing state in chinese", () => {
  assert.equal(deviceLabel({mac:"70:4B:CA:72:AC:A2", name:"BtWriter", remembered:true, authenticated:false}), "BtWriter（已记忆，未认证）");
  assert.equal(deviceLabel({mac:"70:4B:CA:72:AC:A2", name:"BtWriter", remembered:false, authenticated:false}), "BtWriter（未配对）");
  assert.equal(deviceLabel({mac:"41:42:BC:65:9F:A8", name:"ROSE", remembered:true, authenticated:true}), "ROSE（已配对）");
});

test("silent reconnect only targets the remembered mac that is really present", () => {
  assert.equal(silentReconnectTarget(null, []), null);
  assert.equal(silentReconnectTarget("70:4B:CA:72:AC:A2", [{name:"COM13", kind:"usb"}]), null);
  assert.equal(
    silentReconnectTarget("70:4B:CA:72:AC:A2", [{name:"COM7", kind:"bluetooth", mac:"70:4B:CA:72:AC:A2"}]),
    "COM7"
  );
});
