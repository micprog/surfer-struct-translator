package packed_dims_pkg;

  typedef struct packed {
    logic [2:0][1:0] data_q;
    logic [1:0]      state_q;
  } packed_dims_t /* public */;

endpackage

module packed_dims_top;
  import packed_dims_pkg::*;

  packed_dims_t payload_q;
endmodule
