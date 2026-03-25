// Simplified AXI type definitions for testing surfer-struct-gen

package axi_pkg;

  typedef enum logic [1:0] {
    BURST_FIXED = 2'b00,
    BURST_INCR  = 2'b01,
    BURST_WRAP  = 2'b10
  } axi_burst_t /* public */;

  typedef enum logic [1:0] {
    RESP_OKAY   = 2'b00,
    RESP_EXOKAY = 2'b01,
    RESP_SLVERR = 2'b10,
    RESP_DECERR = 2'b11
  } axi_resp_t /* public */;

  typedef struct packed {
    logic [4:0]  id;
    logic [63:0] addr;
    logic [7:0]  len;
    logic [2:0]  size;
    axi_burst_t  burst;
    logic        lock;
    logic [3:0]  cache;
    logic [2:0]  prot;
    logic [3:0]  qos;
    logic [3:0]  region;
    logic [5:0]  atop;
    logic        user;
  } aw_chan_t /* public */;

  typedef struct packed {
    logic [127:0] data;
    logic [15:0]  strb;
    logic         last;
    logic         user;
  } w_chan_t /* public */;

  typedef struct packed {
    logic [4:0]  id;
    axi_resp_t   resp;
    logic        user;
  } b_chan_t /* public */;

  typedef struct packed {
    logic [4:0]  id;
    logic [63:0] addr;
    logic [7:0]  len;
    logic [2:0]  size;
    axi_burst_t  burst;
    logic        lock;
    logic [3:0]  cache;
    logic [2:0]  prot;
    logic [3:0]  qos;
    logic [3:0]  region;
    logic        user;
  } ar_chan_t /* public */;

  typedef struct packed {
    logic [4:0]   id;
    logic [127:0] data;
    axi_resp_t    resp;
    logic         last;
    logic         user;
  } r_chan_t /* public */;

  typedef struct packed {
    aw_chan_t aw;
    logic     aw_valid;
    w_chan_t  w;
    logic     w_valid;
    logic     b_ready;
    ar_chan_t ar;
    logic     ar_valid;
    logic     r_ready;
  } axi_req_t /* public */;

  typedef struct packed {
    logic    aw_ready;
    logic    ar_ready;
    logic    w_ready;
    logic    b_valid;
    b_chan_t b;
    logic    r_valid;
    r_chan_t r;
  } axi_resp_t_full /* public */;

endpackage
