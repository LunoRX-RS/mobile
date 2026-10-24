import type { Metadata, Viewport } from "next";
import type { ReactNode } from "react";
import Script from "next/script";
import "./globals.css";

export const metadata: Metadata = {
  title: "LunoRX",
};

// cover: draw under the notch and home indicator; the shell pads with env(safe-area-inset-*).
export const viewport: Viewport = {
  width: "device-width",
  initialScale: 1,
  viewportFit: "cover",
  themeColor: "#000000",
};

const DEBUG_HOOK = `(function(){
  function show(msg){var d=document.getElementById('lunorx-debug');if(!d){d=document.createElement('div');d.id='lunorx-debug';d.style.cssText='position:fixed;top:70px;left:0;right:0;z-index:99999;background:#300;color:#f88;font:11px/1.35 ui-monospace,monospace;padding:6px;white-space:pre-wrap;word-break:break-all';document.body.appendChild(d);}d.textContent+=msg+"\\n";}
  window.addEventListener('error',function(e){if(e.target&&e.target.tagName==='SCRIPT'){show('SCRIPT FAILED '+e.target.src);return;}show('ERROR: '+(e.message||'')+' @ '+(e.filename||'?')+':'+(e.lineno||0)+'\\n'+((e.error&&e.error.stack)||''));},true);
  window.addEventListener('unhandledrejection',function(e){var r=e.reason;show('REJECT: '+((r&&(r.stack||r.message))||String(r)));});
  document.addEventListener('securitypolicyviolation',function(e){show('CSP '+e.violatedDirective+' blocked='+e.blockedURI);});
  function report(tag){var res=performance.getEntriesByType('resource');var js=res.filter(function(r){return r.initiatorType==='script';});var css=res.filter(function(r){return /\.css/.test(r.name);});show(tag+' js='+js.length+' css='+css.length+' failJs='+js.filter(function(r){return r.responseStatus>=400||r.transferSize===0&&r.decodedBodySize===0;}).length+' next='+(typeof window.next)+' ph='+document.querySelectorAll('.bg-night-canvas').length+' bodyLen='+document.body.textContent.length+' htmlLen='+document.body.innerHTML.length+' main='+document.querySelectorAll('main').length+' scripts='+document.scripts.length);}
  setTimeout(function(){report('T1');},1500);
  setTimeout(function(){report('T5');},5000);
})();`;

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en">
      <body>
        {true && (
          <Script id="lunorx-debug" strategy="beforeInteractive" dangerouslySetInnerHTML={{ __html: DEBUG_HOOK }} />
        )}
        {children}
      </body>
    </html>
  );
}
